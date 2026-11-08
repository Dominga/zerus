//! Setting carry only: no new revision, duration, request or input.
//! The caller retains the state writer and both finite worker rounds. This
//! additional original settings lock is a single try, never a reverse wait.
use super::*;
use crate::state::repositories::directory::Directory;
use fs2::FileExt;

pub(in crate::state) struct Held {
    state: Directory,
    lock: fs::File,
    object: crate::platform::FileIdentity,
}

impl Held {
    pub(in crate::state) fn acquire() -> Result<Self> {
        Self::acquire_mode(true)
    }
    pub(in crate::state) fn acquire_existing() -> Result<Self> {
        Self::acquire_mode(false)
    }
    fn acquire_mode(create: bool) -> Result<Self> {
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        if !state.private()? {
            return Err("The original setting root is not private".into());
        }
        let lock = if create {
            state.open_private_lock("keep-warm.lock")?
        } else {
            use std::os::fd::FromRawFd;
            // SAFETY: one fixed component under this original held root;
            // missing recovery exclusion never creates a replacement file.
            let fd = unsafe {
                libc::openat(
                    state.descriptor(),
                    c"keep-warm.lock".as_ptr(),
                    libc::O_RDWR
                        | libc::O_NOFOLLOW
                        | libc::O_NONBLOCK
                        | libc::O_CLOEXEC
                        | libc::O_NOCTTY,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            // SAFETY: this branch exclusively owns the newly opened descriptor.
            let file = unsafe { fs::File::from_raw_fd(fd) };
            if !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())? {
                return Err("The original keep-warm setting lock is irregular".into());
            }
            file
        };
        lock.try_lock_exclusive()
            .map_err(|_| "The original keep-warm setting is busy")?;
        let object = crate::platform::FileIdentity::of(&lock).map_err(|e| e.to_string())?;
        let held = Self {
            state,
            lock,
            object,
        };
        held.verify()?;
        Ok(held)
    }

    pub(in crate::state) fn verify(&self) -> Result<()> {
        self.state.verify()?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path
            || !self.state.private()?
            || self.lock.metadata().map_err(|e| e.to_string())?.len() != 0
            || crate::platform::FileIdentity::of(&self.lock).map_err(|e| e.to_string())?
                != self.object
        {
            return Err("The original keep-warm setting exclusion changed".into());
        }
        let current = self.state.read_private("keep-warm.lock")?;
        if current.metadata().map_err(|e| e.to_string())?.len() != 0
            || crate::platform::FileIdentity::of(&current).map_err(|e| e.to_string())?
                != self.object
        {
            return Err("The original keep-warm setting lock was replaced".into());
        }
        Ok(())
    }

    pub(in crate::state) fn object(&self) -> Result<(u64, u64)> {
        self.verify()?;
        Ok((self.object.device, self.object.inode))
    }

    fn store(&self) -> Result<Store> {
        self.verify()?;
        // Reuse the complete existing typed validator, not load()'s optional
        // presentation result. Unreadable settings are never an empty store.
        let store = read_store(&self.state.path.join("keep-warm.json"))?;
        self.verify()?;
        Ok(store)
    }

    pub(in crate::state) fn snapshot(&self, before: &Value) -> Result<Option<Entry>> {
        let old = storage::owner_key(before);
        let id = string(before, "session_id");
        if !owner_valid(&old) || !session_id::canonical(id) || old == id {
            return Err("The original keep-warm owner cannot be moved".into());
        }
        let store = self.store()?;
        if store.sessions.contains_key(id) {
            return Err("The session-ID keep-warm setting already exists".into());
        }
        let entry = store.sessions.get(&old).cloned();
        if let Some(entry) = &entry {
            validate(entry, string(before, "name"))?;
        }
        Ok(entry)
    }

    /// Compare ONLY the two original keys against the journal's exact entry.
    /// A later unrelated setting survives recovery; no whole-store rollback.
    pub(in crate::state) fn carry(&self, before: &Value, expected: &Option<Entry>) -> Result<()> {
        let old = storage::owner_key(before);
        let id = string(before, "session_id");
        if !owner_valid(&old) || !session_id::canonical(id) || old == id {
            return Err("The original keep-warm owner cannot be carried".into());
        }
        if let Some(entry) = expected {
            validate(entry, string(before, "name"))?;
        }
        let mut store = self.store()?;
        match (store.sessions.get(&old), store.sessions.get(id), expected) {
            (None, None, None) => return Ok(()),
            (None, Some(current), Some(expected)) if current == expected => return Ok(()),
            (Some(current), None, Some(expected)) if current == expected => (),
            _ => return Err("The original keep-warm entry changed; retain it for review".into()),
        }
        let entry = store
            .sessions
            .remove(&old)
            .ok_or("Missing original keep-warm entry")?;
        store.sessions.insert(id.to_owned(), entry);
        let bytes = format!(
            "{}\n",
            serde_json::to_string(&store).map_err(|e| e.to_string())?
        );
        if bytes.len() as u64 >= MAX_FILE {
            return Err("keep-warm settings exceed limits".into());
        }
        self.verify()?;
        storage::cut_write()?;
        self.state
            .replace_file("keep-warm.json", bytes.as_bytes())?;
        self.verify()?;
        let current = self.store()?;
        if current.sessions != store.sessions {
            return Err("The carried keep-warm settings are unconfirmed".into());
        }
        Ok(())
    }
}

fn validate(entry: &Entry, name: &str) -> Result<()> {
    if entry.name != name
        || name.len() > 2048
        || entry.revision > MAX_REVISION
        || entry.until.is_some_and(|v| !v.is_finite())
        || !entry.set_at.is_finite()
    {
        return Err("The original keep-warm entry is invalid".into());
    }
    Ok(())
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.lock);
    }
}
