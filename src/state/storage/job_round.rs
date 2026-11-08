//! One finite recovery or keep-warm round, never lifetime worker ownership.
//! A metadata transaction may try these same files under the state writer;
//! it must never wait for a worker that can itself need that writer.
use super::*;
use crate::state::repositories::directory::Directory;

#[derive(Clone, Copy)]
pub(in crate::state) enum Kind {
    Recovery,
    KeepWarm,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Recovery => "round.lock",
            Self::KeepWarm => "keep-warm.lock",
        }
    }
}

/// An original lock and its directory objects. No consent, native admission,
/// completion or resource-retirement claim can be made from this value.
pub(in crate::state) struct Held {
    selected: PathBuf,
    state: Directory,
    recovery: Directory,
    file: fs::File,
    identity: crate::platform::FileIdentity,
    kind: Kind,
}

pub(in crate::state) fn try_acquire(kind: Kind) -> Result<Option<Held>> {
    try_at(&root(), kind)
}

/// Existing-original recovery only: missing directories or round files are
/// uncertainty, never a newly created replacement exclusion domain.
#[allow(dead_code)]
pub(in crate::state) fn try_existing(kind: Kind) -> Result<Option<Held>> {
    try_mode(&root(), kind, false)
}

fn try_at(selected: &Path, kind: Kind) -> Result<Option<Held>> {
    try_mode(selected, kind, true)
}
fn try_mode(selected: &Path, kind: Kind, create: bool) -> Result<Option<Held>> {
    if create {
        private_dir(selected)?;
    }
    let state = Directory::open(&crate::platform::canonical(selected).map_err(|e| e.to_string())?)?;
    if !state.owner_controls_entries()? {
        return Err(
            "The current user does not exclusively control the recovery state entries".into(),
        );
    }
    let recovery = if create {
        state.ensure_child("recovery")?
    } else {
        state.child("recovery")?
    };
    if !recovery.owner_controls_entries()? {
        return Err(
            "The current user does not exclusively control the recovery round entries".into(),
        );
    }
    let file = if create {
        recovery.open_private_lock(kind.name())?
    } else {
        existing(&recovery, kind.name())?
    };
    match file.try_lock_exclusive() {
        Ok(()) => (),
        Err(error) if crate::platform::lock_contended(&error) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let identity = crate::platform::FileIdentity::of(&file).map_err(|e| e.to_string())?;
    let held = Held {
        selected: selected.to_owned(),
        state,
        recovery,
        file,
        identity,
        kind,
    };
    held.verify_at(selected)?;
    Ok(Some(held))
}

impl Held {
    pub(in crate::state) fn object(&self) -> Result<(u64, u64)> {
        self.verify()?;
        Ok((self.identity.device, self.identity.inode))
    }
    pub(in crate::state) fn verify(&self) -> Result<()> {
        self.verify_at(&root())
    }

    fn verify_at(&self, selected: &Path) -> Result<()> {
        self.state.verify()?;
        self.recovery.verify()?;
        if selected != self.selected
            || crate::platform::canonical(selected).map_err(|e| e.to_string())? != self.state.path
            || !self.state.owner_controls_entries()?
            || !self.recovery.owner_controls_entries()?
        {
            return Err("The original recovery round root changed".into());
        }
        let current = self.recovery.read_private(self.kind.name())?;
        if self.file.metadata().map_err(|e| e.to_string())?.len() != 0
            || current.metadata().map_err(|e| e.to_string())?.len() != 0
            || crate::platform::FileIdentity::of(&self.file).map_err(|e| e.to_string())?
                != self.identity
            || crate::platform::FileIdentity::of(&current).map_err(|e| e.to_string())?
                != self.identity
        {
            return Err("The original recovery round file changed".into());
        }
        Ok(())
    }
}

fn existing(directory: &Directory, name: &str) -> Result<fs::File> {
    use std::os::fd::FromRawFd;
    let name = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
    directory.verify()?;
    // SAFETY: the kind supplies one fixed name under the original directory.
    // No creation, symlink follow or blocking FIFO open is requested.
    let fd = unsafe {
        libc::openat(
            directory.descriptor(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: this call exclusively owns the newly returned descriptor.
    let file = unsafe { fs::File::from_raw_fd(fd) };
    if !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())? {
        return Err("The original recovery round is not a private regular file".into());
    }
    directory.verify()?;
    Ok(file)
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod compatibility;

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::time::{Duration, Instant};

    #[test]
    fn a_round_stays_excluded_until_its_original_file_is_dropped() {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("state");
        for kind in [Kind::Recovery, Kind::KeepWarm] {
            let held = try_at(&selected, kind).unwrap().unwrap();
            assert!(try_at(&selected, kind).unwrap().is_none());
            let original = held.identity;
            held.verify_at(&selected).unwrap();
            drop(held);
            let next = try_at(&selected, kind).unwrap().unwrap();
            assert!(next.identity == original);
        }
    }

    #[test]
    fn legacy_readable_directories_keep_their_mode_but_shared_writers_refuse() {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("state");
        fs::create_dir(&selected).unwrap();
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o755)).unwrap();
        let held = try_at(&selected, Kind::Recovery).unwrap().unwrap();
        held.verify_at(&selected).unwrap();
        assert!(
            !held.state.private().unwrap(),
            "this is never a native private-root proof"
        );
        use std::os::unix::fs::MetadataExt;
        assert_eq!(fs::metadata(&selected).unwrap().mode() & 0o777, 0o755);
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(held.verify_at(&selected).is_err());
        drop(held);
        assert!(try_at(&selected, Kind::Recovery).is_err());
        assert_eq!(fs::metadata(&selected).unwrap().mode() & 0o777, 0o775);
    }

    #[test]
    fn a_writer_never_waits_for_a_round_that_itself_needs_the_writer() {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("state");
        private_dir(&selected).unwrap();
        let writer_path = selected.join("writer.lock");
        let writer = fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&writer_path)
            .unwrap();
        writer.try_lock_exclusive().unwrap();
        let root = selected.clone();
        let (send, ready) = std::sync::mpsc::sync_channel(1);
        let child = std::thread::spawn(move || {
            let _round = try_at(&root, Kind::Recovery).unwrap().unwrap();
            let waiting = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(writer_path)
                .unwrap();
            send.send(()).unwrap();
            let until = Instant::now() + Duration::from_secs(5);
            while waiting.try_lock_exclusive().is_err() {
                if Instant::now() >= until {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            true
        });
        let entered = ready.recv_timeout(Duration::from_secs(2)).is_ok();
        let start = Instant::now();
        let refused = entered && matches!(try_at(&selected, Kind::Recovery), Ok(None));
        let elapsed = start.elapsed();
        drop(writer);
        let settled = child.join().unwrap();
        // Every original is released and joined before asserting the result.
        assert!(entered && refused && settled);
        assert!(elapsed < Duration::from_secs(2));
    }

    #[test]
    fn replacement_and_root_redirect_invalidate_the_original_round() {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("selected");
        let held = try_at(&selected, Kind::Recovery).unwrap().unwrap();
        let lock = selected.join("recovery/round.lock");
        fs::rename(&lock, lock.with_extension("old")).unwrap();
        fs::write(&lock, b"").unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(held.verify_at(&selected).is_err());
        fs::rename(&selected, temp.path().join("original")).unwrap();
        let foreign = temp.path().join("foreign");
        fs::create_dir(&foreign).unwrap();
        symlink(&foreign, &selected).unwrap();
        assert!(held.verify_at(&selected).is_err());
        assert!(fs::read_dir(&foreign).unwrap().next().is_none());
    }

    #[test]
    fn a_fifo_or_alias_is_refused_without_waiting_or_a_foreign_write() {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("state");
        let held = try_at(&selected, Kind::Recovery).unwrap().unwrap();
        drop(held);
        let lock = selected.join("recovery/round.lock");
        fs::remove_file(&lock).unwrap();
        let name = std::ffi::CString::new(lock.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: this test owns the private, NUL-free fixture pathname.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let start = Instant::now();
        assert!(try_at(&selected, Kind::Recovery).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        fs::remove_file(&lock).unwrap();
        let foreign = temp.path().join("foreign");
        symlink(&foreign, &lock).unwrap();
        assert!(try_at(&selected, Kind::Recovery).is_err());
        assert!(!foreign.exists());
    }
}
