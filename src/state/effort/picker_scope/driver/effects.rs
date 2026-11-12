//! Recovery's fixed filesystem effects under the SAME original owner. No
//! ordinary atomic/remove/restore implementation is a fallback in this lane.
use super::*;
use crate::state::guarded_recovery::{Effects, Original as Source};
use std::ffi::CString;

pub(super) struct Recovery(pub(super) Arc<owner::Owner>);

fn component(path: &std::path::Path) -> Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    let name = path.file_name().ok_or("Recovery effect has no filename")?;
    CString::new(name.as_bytes()).map_err(|e| e.to_string())
}

fn sources(owner: &owner::Owner, proofs: &[&Source]) -> Result<()> {
    owner.actor()?;
    owner.check()?;
    for proof in proofs {
        proof.verify()?;
    }
    Ok(())
}

fn boundary(owner: &owner::Owner, proofs: &[&Source]) -> Result<()> {
    sources(owner, proofs)?;
    owner.control.commit_boundary()
}

pub(super) fn parent(
    owner: &owner::Owner,
    path: &std::path::Path,
    proofs: &[&Source],
) -> Result<Directory> {
    let path = path.parent().ok_or("Recovery effect has no parent")?;
    if !path.is_absolute() {
        return Err("Recovery effect is not absolute".into());
    }
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    existing
                        .file_name()
                        .ok_or("Invalid recovery parent")?
                        .to_owned(),
                );
                existing = existing.parent().ok_or("Missing recovery namespace")?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let canonical = crate::platform::canonical(existing).map_err(|e| e.to_string())?;
    let mut held = Directory::open(&canonical)?;
    for name in missing.into_iter().rev() {
        held.verify()?;
        if !held.owner_controls_entries()? {
            return Err("Recovery parent entries are not controlled by this owner".into());
        }
        let entry = component(std::path::Path::new(&name))?;
        boundary(owner, proofs)?;
        // One admitted mkdir under the retained parent. No recursive pathname
        // fallback and no renewed deadline during a blocked syscall/sync.
        let result = unsafe { libc::mkdirat(held.descriptor(), entry.as_ptr(), 0o700) };
        if result != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        held.sync()?;
        held = held.child(name.to_str().ok_or("Non-UTF8 recovery parent")?)?;
    }
    if crate::platform::canonical(path).map_err(|e| e.to_string())? != held.path {
        return Err("Recovery parent route changed".into());
    }
    held.verify()?;
    if !held.owner_controls_entries()? {
        return Err("Recovery parent entries are not controlled by this owner".into());
    }
    Ok(held)
}

fn target<'a>(path: &std::path::Path, proofs: &'a [&Source]) -> Result<&'a Source> {
    proofs
        .iter()
        .copied()
        .find(|proof| proof.path() == path)
        .ok_or("Recovery effect has no original destination proof".into())
}

impl Recovery {
    fn result<T>(&self, result: Result<T>) -> Result<T> {
        result.or_else(|error| self.0.refuse(error))
    }
}

impl Effects for Recovery {
    fn check(&self) -> Result<()> {
        self.result(self.0.actor().and_then(|()| self.0.check()))
    }
    fn until(&self) -> Instant {
        self.0.until
    }
    fn read(&self, path: &std::path::Path, limit: u64) -> Result<Option<Source>> {
        self.result((|| {
            self.check()?;
            let source = Source::read_existing(path, limit)?;
            self.check()?;
            Ok(source)
        })())
    }
    fn session_name(&self, id: &str) -> Result<Option<String>> {
        self.result((|| {
            let args = ["display-message", "-p", "-t", id, "#{session_name}"].map(str::to_owned);
            let output = read::invoke(&self.0, read::Call::SessionName, &args)?;
            Ok(output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
        })())
    }
    fn atomic(&self, path: &std::path::Path, bytes: &[u8], proofs: &[&Source]) -> Result<()> {
        self.result((|| {
            sources(&self.0, proofs)?;
            let original = target(path, proofs)?;
            let parent = parent(&self.0, path, proofs)?;
            let staged = publication::Staged::prepare(
                &parent,
                path.file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("Invalid recovery name")?,
                bytes,
            )?;
            if staged.expected_version() != original.version() {
                return Err("Recovery staged destination differs from original source".into());
            }
            let fence = publication::CommitFence::recovery(&self.0, &staged, proofs)?;
            staged.commit(&fence).map(drop)
        })())
    }
    fn remove(&self, path: &std::path::Path, proofs: &[&Source]) -> Result<()> {
        self.result((|| {
            let original = target(path, proofs)?;
            if !original.is_present() {
                return boundary(&self.0, proofs);
            }
            let parent = parent(&self.0, path, proofs)?;
            let name = component(path)?;
            crate::state::storage::cut_write()?;
            parent.verify()?;
            boundary(&self.0, proofs)?;
            if unsafe { libc::unlinkat(parent.descriptor(), name.as_ptr(), 0) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            parent.sync()
        })())
    }
    fn restore(
        &self,
        lock: &std::path::Path,
        taken: &std::path::Path,
        sid: &str,
        object: Option<&Value>,
        proofs: &[&Source],
    ) -> Result<()> {
        self.result((|| {
            if lock.parent() != taken.parent() {
                return Err("Recovery protective lock parents differ".into());
            }
            let original = target(taken, proofs)?;
            let Some(file) = original.file() else {
                return boundary(&self.0, proofs);
            };
            let destination = target(lock, proofs)?;
            let held = parent(&self.0, lock, proofs)?;
            let from = component(taken)?;
            let to = component(lock)?;
            let identity = crate::platform::FileIdentity::of(file).map_err(|e| e.to_string())?;
            let expected_object = json!([identity.device, identity.inode]);
            let exact_text = std::str::from_utf8(original.bytes()).is_ok_and(|text| {
                text.strip_suffix('\n').unwrap_or(text) == format!("zerus:{sid}")
            });
            crate::state::storage::cut_write()?;
            held.verify()?;
            boundary(&self.0, proofs)?;
            let linked = unsafe {
                libc::linkat(
                    held.descriptor(),
                    from.as_ptr(),
                    held.descriptor(),
                    to.as_ptr(),
                    0,
                )
            } == 0;
            if !linked {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error.to_string());
                }
                let same = destination.file().is_some_and(|other| {
                    crate::platform::FileIdentity::of(other).is_ok_and(|other| {
                        (other.device, other.inode) == (identity.device, identity.inode)
                    })
                });
                if !same
                    && (!exact_text || object.is_some_and(|expected| *expected != expected_object))
                {
                    return Err(
                        "Another protective lock appeared; original taken lock stays for review"
                            .into(),
                    );
                }
            }
            // The known successful link changes ONLY this SAME opened inode's
            // ctime/link count. Retain that file and derive its new version;
            // never refresh an arbitrary pathname after an error.
            let before = original.version().ok_or("Original taken version missing")?;
            let after = if linked {
                // Capture before any blocking namespace sync, and admit only
                // the known own link's ctime change on this SAME open file.
                let after = version(file)?;
                if before.as_array().ok_or("Invalid taken version")?[..8]
                    != after.as_array().ok_or("Invalid taken version")?[..8]
                {
                    return Err("Taken protective lock changed during own restore link".into());
                }
                after
            } else {
                // EEXIST made no own inode transition. The original version
                // remains exact; a later sync can never authorize a rebase.
                before.clone()
            };
            #[cfg(test)]
            tests::restore_sync_fault();
            held.sync()?;
            let others: Vec<_> = proofs
                .iter()
                .copied()
                .filter(|proof| !linked || (proof.path() != taken && proof.path() != lock))
                .collect();
            sources(&self.0, &others)?;
            held.verify()?;
            if version(file)? != after
                || named_version(&held, &from)? != after
                || (linked && named_version(&held, &to)? != after)
            {
                return Err(
                    "Own restored protective lock changed before taken-name removal".into(),
                );
            }
            crate::state::storage::cut_write()?;
            self.0.control.commit_boundary()?;
            if unsafe { libc::unlinkat(held.descriptor(), from.as_ptr(), 0) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            held.sync()
        })())
    }
}

fn named_version(parent: &Directory, name: &CString) -> Result<Value> {
    use std::os::fd::FromRawFd;
    let raw = unsafe {
        libc::openat(
            parent.descriptor(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let file = unsafe { File::from_raw_fd(raw) };
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Restored protective lock is not regular".into());
    }
    version(&file)
}
