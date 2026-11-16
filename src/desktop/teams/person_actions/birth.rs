//! First Create binds existing directory objects, then creates only an absent
//! database. It never adopts a file which appeared after the native review.
use super::required;
use crate::{platform, teams::store};
use std::{fs::File, path::Path};
use zerus_client_core::desktop::team_person_actions as wire;
use zerus_team_plan::Problem;
type Result<T> = std::result::Result<T, Problem>;

#[cfg(unix)]
fn folder(path: &Path) -> Result<File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(store::storage)?;
    let metadata = file.metadata().map_err(store::storage)?;
    // SAFETY: geteuid has no pointer arguments or ownership requirements.
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(required());
    }
    Ok(file)
}

pub(super) fn root(state: &Path) -> Result<String> {
    #[cfg(unix)]
    {
        let state = platform::canonical(state).map_err(store::storage)?;
        let parent = folder(&state)?;
        let teams = folder(&state.join("teams"))?;
        let a = platform::FileIdentity::of(&parent).map_err(store::storage)?;
        let b = platform::FileIdentity::of(&teams).map_err(store::storage)?;
        wire::digest(&(state, a.device, a.inode, b.device, b.inode)).ok_or_else(required)
    }
    #[cfg(not(unix))]
    {
        let _ = state;
        Err(required())
    }
}

pub(super) struct Birth {
    file: File,
    root: String,
}
impl Birth {
    /// Called only after the exact live native original passed preflight.
    pub fn create(state: &Path, expected: &str) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::{AsRawFd, FromRawFd};
            if root(state)? != expected {
                return Err(required());
            }
            let directory = folder(&state.join("teams"))?;
            // The literal child is created relative to the reviewed open
            // directory, with an atomic absence test, never truncate/open.
            // SAFETY: the owned directory descriptor stays live; the child
            // name is a fixed NUL-terminated literal and flags are constant.
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    c"teams-v1.sqlite3".as_ptr(),
                    libc::O_RDWR
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                let error = std::io::Error::last_os_error();
                return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
                    required()
                } else {
                    store::storage(error)
                });
            }
            // SAFETY: openat returned this one new owned descriptor.
            let file = unsafe { File::from_raw_fd(fd) };
            file.sync_all().map_err(store::storage)?;
            directory.sync_all().map_err(store::storage)?;
            let born = Self {
                file,
                root: expected.to_owned(),
            };
            born.verify(state)?;
            Ok(born)
        }
        #[cfg(not(unix))]
        {
            let _ = (state, expected);
            Err(required())
        }
    }

    pub fn verify(&self, state: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            if root(state)? != self.root {
                return Err(required());
            }
            let held = platform::FileIdentity::of(&self.file).map_err(store::storage)?;
            let current = crate::sqlite_file::private(&state.join("teams/teams-v1.sqlite3"))
                .map_err(store::storage)?;
            if held.device != current.device || held.inode != current.inode {
                return Err(required());
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = state;
            Err(required())
        }
    }
}
