//! The veto retained before a new ID-layout record is published. These
//! objects cannot authorize removal or a signal. The caller holds the state
//! writer; repository admission is a nonblocking try in that lock order.
use super::*;
use crate::state::repositories::directory::Directory;
use fs2::FileExt;
use std::io::{Seek, SeekFrom, Write};

pub(in crate::state) struct Protection {
    owner: Value,
    state: Directory,
    common: Directory,
    checkout: Option<Directory>,
    metadata: Directory,
    _repository: fs::File,
    receipt: PathBuf,
    before: Value,
    lock: fs::File,
    reason: String,
}

fn object(file: &fs::File) -> Result<Value> {
    let id = crate::platform::FileIdentity::of(file).map_err(|e| e.to_string())?;
    Ok(json!([id.device, id.inode]))
}

fn state() -> Result<Directory> {
    Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)
}

fn receipt(state: &Directory, owner: &Value) -> PathBuf {
    state
        .path
        .join("worktree-protections")
        .join(format!("{}.json", storage::label_key(string(owner, "id"))))
}

fn read_receipt(path: &Path) -> Result<Option<Value>> {
    labels::read_file(path, 16384)
        .map_err(|e| e.to_string())?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
        .transpose()
}

fn owner_matches(owner: &Value) -> bool {
    let path = Path::new(string(owner, "path"));
    let common = Path::new(string(owner, "common_dir"));
    owner["version"] == 1
        && path.is_absolute()
        && common.is_absolute()
        && session_id::canonical(string(owner, "id"))
        && worktree_lifecycle::owned(common, path).as_ref() == Some(owner)
}

fn open_lock(metadata: &Directory) -> Result<fs::File> {
    let file = crate::platform::no_follow(fs::OpenOptions::new().read(true).write(true))
        .open(metadata.path.join("locked"))
        .map_err(|e| e.to_string())?;
    // A hard link could mutate an unrelated object through this descriptor.
    if !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())? {
        return Err("The protective lock is not a private regular file".into());
    }
    Ok(file)
}

fn text(file: &mut fs::File) -> Result<String> {
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(LOCK_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LOCK_BYTES {
        return Err("The protective lock exceeds its bound".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

impl Protection {
    /// Existing protection only. A missing lock with a retained Ready receipt
    /// is a person's unlock or an interrupted action, never a relock request.
    fn retained(owner: &Value, expected: Option<&str>, allow_missing: bool) -> Result<Self> {
        if !owner_matches(owner) {
            return Err("The original checkout ownership changed".into());
        }
        let common = Directory::open(Path::new(string(owner, "common_dir")))?;
        let checkout = match Directory::open(Path::new(string(owner, "path"))) {
            Ok(checkout) => Some(checkout),
            Err(_)
                if allow_missing
                    && fs::symlink_metadata(string(owner, "path"))
                        .is_err_and(|e| e.kind() == io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(error) => return Err(error),
        };
        let metadata = Directory::open(Path::new(string(owner, "metadata")))?;
        let repository = worktree_cleanup::repository_lock(&common.path)?;
        repository
            .try_lock_exclusive()
            .map_err(|_| "The worktree is busy; nothing was published")?;
        let state = state()?;
        let receipt = receipt(&state, owner);
        let before = read_receipt(&receipt)?
            .ok_or("An existing Git lock is not Zerus publication protection")?;
        if before["version"] != 1
            || before["status"] != "ready"
            || before["owner"] != *owner
            || before["state_object"] != json!(state.object()?)
            || before["common_object"] != json!(common.object()?)
            || match &checkout {
                Some(checkout) => before["checkout_object"] != json!(checkout.object()?),
                None => !before["checkout_object"]
                    .as_array()
                    .is_some_and(|a| a.len() == 2 && a.iter().all(|n| n.as_u64().is_some())),
            }
            || before["metadata_object"] != json!(metadata.object()?)
        {
            return Err("The original protective lock needs manual review".into());
        }
        let mut lock = open_lock(&metadata)?;
        if before["lock_object"] != object(&lock)? {
            return Err("The protective lock was replaced".into());
        }
        let reason = text(&mut lock)?
            .strip_prefix("zerus:")
            .and_then(|s| s.strip_suffix('\n'))
            .filter(|id| session_id::canonical(id))
            .ok_or("The protective lock changed")?
            .to_owned();
        if expected.is_some_and(|id| id != reason) {
            return Err("The protective lock belongs to another session".into());
        }
        let value = Self {
            owner: owner.clone(),
            state,
            common,
            checkout,
            metadata,
            _repository: repository,
            receipt,
            before,
            lock,
            reason,
        };
        value.verify()?;
        Ok(value)
    }

    /// Called before create-only publication, under the existing writer. No
    /// owner means a custom/plain folder: no Git command or lock is attempted.
    pub(in crate::state) fn acquire(record: &Value) -> Result<Option<Self>> {
        let owner = &record["worktree_owner"];
        if owner.is_null() {
            return Ok(None);
        }
        let id = string(record, "session_id");
        if !session_id::canonical(id)
            || !owner_matches(owner)
            || !Path::new(string(record, "launch_dir"))
                .starts_with(Path::new(string(owner, "path")))
        {
            return Err("Cannot protect the original owned checkout for this session".into());
        }
        let refs = worktree_cleanup::bindings()?;
        let path = Path::new(string(owner, "path"));
        let users: Vec<_> = refs
            .iter()
            .filter(|r| {
                r.owner == string(owner, "id")
                    || r.folders.iter().any(|folder| folder.starts_with(path))
            })
            .collect();
        let state = state()?;
        let receipt = receipt(&state, owner);
        let current = read_receipt(&receipt)?;
        if current.as_ref().is_some_and(|r| r["status"] == "ready") {
            let held = Self::retained(owner, None, false)?;
            // An orphaned lock from interrupted publication is never adopted.
            if !users
                .iter()
                .any(|r| r.owner == string(owner, "id") && r.session_id == held.reason)
            {
                return Err(
                    "The protective lock has no verified live or saved holder; review it manually"
                        .into(),
                );
            }
            return Ok(Some(held));
        }
        let common = Directory::open(Path::new(string(owner, "common_dir")))?;
        let checkout = Directory::open(path)?;
        let metadata = Directory::open(Path::new(string(owner, "metadata")))?;
        let repository = worktree_cleanup::repository_lock(&common.path)?;
        repository
            .try_lock_exclusive()
            .map_err(|_| "The worktree is busy; nothing was published")?;
        match fs::symlink_metadata(metadata.path.join("locked")) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            _ => {
                return Err(
                    "This checkout already has a Git lock; Zerus will not replace it".into(),
                )
            }
        }
        let mut before = json!({"version":1,"status":"preparing","owner":owner,"state_object":state.object()?,
            "common_object":common.object()?,"checkout_object":checkout.object()?,"metadata_object":metadata.object()?});
        match current {
            None => storage::atomic_new(&receipt, &before.to_string())?,
            Some(previous)
                if previous["status"] == "released"
                    && previous["owner"] == *owner
                    && previous["state_object"] == before["state_object"]
                    && previous["common_object"] == before["common_object"]
                    && previous["checkout_object"] == before["checkout_object"]
                    && previous["metadata_object"] == before["metadata_object"] =>
            {
                atomic(&receipt, &before.to_string())?
            }
            _ => return Err(
                "An interrupted protection or manual unlock needs review; Zerus will not relock it"
                    .into(),
            ),
        }
        common.verify()?;
        checkout.verify()?;
        metadata.verify()?;
        if !owner_matches(owner) {
            return Err("Checkout ownership changed before protection".into());
        }
        storage::cut_write()?;
        let lock = metadata.write_new_file("locked", format!("zerus:{id}\n").as_bytes())?;
        crate::platform::sync_dir(&metadata.path).map_err(|e| e.to_string())?;
        before["lock_object"] = object(&lock)?;
        before["status"] = json!("ready");
        atomic(&receipt, &before.to_string())?;
        let held = Self {
            owner: owner.clone(),
            state,
            common,
            checkout: Some(checkout),
            metadata,
            _repository: repository,
            receipt,
            before,
            lock,
            reason: id.into(),
        };
        held.verify()?;
        Ok(Some(held))
    }

    pub(in crate::state) fn verify(&self) -> Result<()> {
        self.state.verify()?;
        self.common.verify()?;
        self.checkout_current()?;
        self.metadata.verify()?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path {
            return Err("The selected state root changed".into());
        }
        if !owner_matches(&self.owner)
            || read_receipt(&self.receipt)?.as_ref() != Some(&self.before)
        {
            return Err("Checkout ownership or protection changed".into());
        }
        let mut current = open_lock(&self.metadata)?;
        if object(&current)? != object(&self.lock)?
            || object(&current)? != self.before["lock_object"]
            || text(&mut current)? != format!("zerus:{}\n", self.reason)
        {
            return Err("The original protective lock changed; nothing was published".into());
        }
        Ok(())
    }

    pub(super) fn original_object(&self) -> Result<Value> {
        object(&self.lock)
    }

    fn checkout_current(&self) -> Result<()> {
        match &self.checkout {
            Some(checkout) => checkout.verify(),
            // Release after a person removed the checkout still checks the
            // original metadata/nonce/backlink and complete reference census.
            // This branch is unavailable to publication and cannot relock it.
            None if fs::symlink_metadata(string(&self.owner, "path"))
                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound) =>
            {
                Ok(())
            }
            None => Err("The missing checkout was replaced".into()),
        }
    }

    /// Keep the same regular Git lock present throughout a shared handoff.
    /// Partial/error writes leave a lock for review, never an unlocked gap.
    pub(super) fn transfer(mut self, id: &str) -> Result<()> {
        if !session_id::canonical(id) {
            return Err("A shared holder has no stable session identity".into());
        }
        self.verify()?;
        storage::cut_write()?;
        self.lock
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        self.lock
            .write_all(format!("zerus:{id}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        self.lock.sync_all().map_err(|e| e.to_string())?;
        self.reason = id.into();
        self.verify()
    }

    pub(super) fn released(&self) -> Result<()> {
        self.state.verify()?;
        self.common.verify()?;
        self.checkout_current()?;
        self.metadata.verify()?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path {
            return Err("The selected state root changed".into());
        }
        if read_receipt(&self.receipt)?.as_ref() != Some(&self.before)
            || !fs::symlink_metadata(self.metadata.path.join("locked"))
                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            || !owner_matches(&self.owner)
        {
            return Err("The protective release is unconfirmed".into());
        }
        let mut released = self.before.clone();
        released["status"] = json!("released");
        atomic(&self.receipt, &released.to_string())
    }
}

/// None is the old floor's release-only lock, without publication metadata.
/// Present but changed metadata is never silently treated as that old case.
pub(super) fn existing(record: &Value) -> Result<Option<Protection>> {
    let owner = &record["worktree_owner"];
    let state = state()?;
    if read_receipt(&receipt(&state, owner))?.is_none() {
        return Ok(None);
    }
    Protection::retained(owner, Some(string(record, "session_id")), true).map(Some)
}
