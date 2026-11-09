//! Read-only, descriptor-based checkout membership for an explicit program
//! review. An object match is evidence only; this module cannot send signals.
use std::{
    collections::BTreeSet,
    ffi::{CStr, CString, OsStr},
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd, RawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path, PathBuf},
    time::Instant,
};

const MAX_OBJECTS: usize = 65_536;
const MAX_DEPTH: usize = 128;
const MAX_NAMES: usize = 8 << 20;

#[allow(dead_code)] // Object transfer is used only by the closed live holder protocol.
pub(crate) mod loan;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Object {
    pub device: u64,
    pub inode: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    name: PathBuf,
    object: Object,
    kind: libc::mode_t,
    // A second complete walk refuses changes during the process census. A
    // timestamp is never used as the positive membership or process identity.
    changed: (i64, i64),
    modified: (i64, i64),
}

fn invalid() -> io::Error {
    io::Error::other("the checkout object inventory changed or is incomplete")
}
fn check(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "checkout inventory deadline",
        ))
    } else {
        Ok(())
    }
}
fn stat(fd: RawFd) -> io::Result<libc::stat> {
    let mut value = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut value) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(value)
}
#[allow(clippy::unnecessary_cast)]
pub(super) fn object(stat: &libc::stat) -> Object {
    #[cfg(target_os = "macos")]
    let device = u64::from(stat.st_dev as u32);
    #[cfg(target_os = "linux")]
    let device = stat.st_dev as u64;
    Object {
        device,
        inode: stat.st_ino as u64,
    }
}
fn entry(name: PathBuf, stat: &libc::stat) -> Entry {
    Entry {
        name,
        object: object(stat),
        kind: stat.st_mode & libc::S_IFMT,
        changed: (stat.st_ctime, stat.st_ctime_nsec),
        modified: (stat.st_mtime, stat.st_mtime_nsec),
    }
}
fn directory(at: RawFd, name: &CStr) -> io::Result<File> {
    let fd = unsafe {
        libc::openat(
            at,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn name(value: &OsStr) -> io::Result<CString> {
    Ok(CString::new(value.as_bytes())?)
}

#[cfg(target_os = "linux")]
fn mount(fd: RawFd) -> io::Result<String> {
    mount_at(fd, c"", libc::AT_EMPTY_PATH | libc::AT_SYMLINK_NOFOLLOW)
}
#[cfg(target_os = "linux")]
fn mount_at(at: RawFd, name: &CStr, flags: libc::c_int) -> io::Result<String> {
    let mut value: libc::statx = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::statx(
            at,
            name.as_ptr(),
            flags,
            libc::STATX_MNT_ID | libc::STATX_MNT_ID_UNIQUE,
            &mut value,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if value.stx_mask & libc::STATX_MNT_ID_UNIQUE != 0 {
        Ok(format!("unique:{}", value.stx_mnt_id))
    } else if value.stx_mask & libc::STATX_MNT_ID != 0 {
        Ok(format!("id:{}", value.stx_mnt_id))
    } else {
        Err(invalid())
    }
}
#[cfg(target_os = "macos")]
fn mount(fd: RawFd) -> io::Result<String> {
    let mut value: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(fd, &mut value) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let bytes = unsafe {
        std::slice::from_raw_parts(
            (&value.f_fsid as *const libc::fsid_t).cast::<u8>(),
            std::mem::size_of::<libc::fsid_t>(),
        )
    };
    let fsid: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{}:{}",
        fsid,
        unsafe { CStr::from_ptr(value.f_mntonname.as_ptr()) }.to_string_lossy()
    ))
}

/// Root and every original name in its ancestry stay pinned. No path prefix,
/// symlink target or process-reported filename is accepted as membership.
pub(crate) struct Scope {
    path: PathBuf,
    ancestors: Vec<(File, CString, Object)>,
    root: File,
    identity: zerus_client_core::worktree_programs::DirectoryId,
}
impl Scope {
    pub(crate) fn open(path: &Path, deadline: Instant) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(invalid());
        }
        let mut current = directory(libc::AT_FDCWD, c"/")?;
        let mut ancestors = Vec::new();
        for component in path.components() {
            check(deadline)?;
            let Component::Normal(part) = component else {
                if component == Component::RootDir {
                    continue;
                }
                return Err(invalid());
            };
            if ancestors.len() >= MAX_DEPTH {
                return Err(invalid());
            }
            let part = name(part)?;
            let next = directory(current.as_raw_fd(), &part)?;
            let id = object(&stat(next.as_raw_fd())?);
            ancestors.push((current, part, id));
            current = next;
        }
        if ancestors.is_empty() {
            return Err(invalid());
        }
        let id = object(&stat(current.as_raw_fd())?);
        let identity = zerus_client_core::worktree_programs::DirectoryId {
            device: id.device,
            inode: id.inode,
            mount: mount(current.as_raw_fd())?,
        };
        let scope = Self {
            path: path.into(),
            ancestors,
            root: current,
            identity,
        };
        scope.current(deadline)?;
        Ok(scope)
    }
    pub(crate) fn identity(&self) -> &zerus_client_core::worktree_programs::DirectoryId {
        &self.identity
    }
    /// Native exclusion evidence for the custodian's own data/cwd, never
    /// signal authority. Its deliberately held checkout pin is not a target.
    pub(crate) fn protects(&self, paths: &[&Path], deadline: Instant) -> io::Result<bool> {
        use std::os::unix::fs::MetadataExt;
        let inventory = self.inventory(deadline)?;
        let mut found = false;
        for path in paths {
            check(deadline)?;
            let metadata = std::fs::metadata(path)?;
            #[cfg(target_os = "linux")]
            let device = metadata.dev();
            #[cfg(target_os = "macos")]
            let device = u64::from(metadata.dev() as u32);
            found |= inventory.contains(Object {
                device,
                inode: metadata.ino(),
            });
        }
        inventory.current(self, deadline)?;
        Ok(found)
    }
    pub(crate) fn current(&self, deadline: Instant) -> io::Result<()> {
        for (parent, part, expected) in &self.ancestors {
            check(deadline)?;
            let child = directory(parent.as_raw_fd(), part)?;
            if object(&stat(child.as_raw_fd())?) != *expected {
                return Err(invalid());
            }
        }
        if mount(self.root.as_raw_fd())? != self.identity.mount {
            return Err(invalid());
        }
        // The stored absolute spelling is data for original action binding;
        // its native component observations above establish current ownership.
        if !self.path.is_absolute() {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn inventory(&self, deadline: Instant) -> io::Result<Inventory> {
        self.current(deadline)?;
        let mut entries = Vec::new();
        let mut names = 0;
        self.walk(
            &self.root,
            Path::new(""),
            0,
            &mut entries,
            &mut names,
            deadline,
        )?;
        self.current(deadline)?;
        entries.sort();
        let objects = entries.iter().map(|row| row.object).collect();
        Ok(Inventory { entries, objects })
    }
    fn walk(
        &self,
        folder: &File,
        relative: &Path,
        depth: usize,
        rows: &mut Vec<Entry>,
        names: &mut usize,
        deadline: Instant,
    ) -> io::Result<()> {
        check(deadline)?;
        if depth >= MAX_DEPTH
            || rows.len() >= MAX_OBJECTS
            || mount(folder.as_raw_fd())? != self.identity.mount
        {
            return Err(invalid());
        }
        let before = stat(folder.as_raw_fd())?;
        rows.push(entry(relative.into(), &before));
        // A separate open file description is essential: dup would share the
        // directory offset and silently make a repeated inventory empty.
        let copy = directory(folder.as_raw_fd(), c".")?;
        let raw = std::os::fd::IntoRawFd::into_raw_fd(copy);
        let stream = unsafe { libc::fdopendir(raw) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            unsafe {
                libc::close(raw);
            }
            return Err(error);
        }
        struct Stream(*mut libc::DIR);
        impl Drop for Stream {
            fn drop(&mut self) {
                unsafe {
                    libc::closedir(self.0);
                }
            }
        }
        let stream = Stream(stream);
        loop {
            check(deadline)?;
            #[cfg(target_os = "linux")]
            unsafe {
                *libc::__errno_location() = 0;
            }
            #[cfg(target_os = "macos")]
            unsafe {
                *libc::__error() = 0;
            }
            let row = unsafe { libc::readdir(stream.0) };
            if row.is_null() {
                if io::Error::last_os_error().raw_os_error() != Some(0) {
                    return Err(io::Error::last_os_error());
                }
                break;
            }
            let part = unsafe { CStr::from_ptr((*row).d_name.as_ptr()) };
            if [b".".as_slice(), b"..".as_slice()].contains(&part.to_bytes()) {
                continue;
            }
            let mut metadata: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe {
                libc::fstatat(
                    folder.as_raw_fd(),
                    part.as_ptr(),
                    &mut metadata,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            let relative = relative.join(OsStr::from_bytes(part.to_bytes()));
            // Bound retained full relative names, including their repeated
            // ancestry, rather than only leaf bytes in a deep tree.
            *names = names
                .checked_add(relative.as_os_str().len())
                .ok_or_else(invalid)?;
            if *names > MAX_NAMES || rows.len() >= MAX_OBJECTS {
                return Err(invalid());
            }
            if metadata.st_mode & libc::S_IFMT == libc::S_IFDIR {
                let child = directory(folder.as_raw_fd(), part)?;
                if object(&stat(child.as_raw_fd())?) != object(&metadata) {
                    return Err(invalid());
                }
                self.walk(&child, &relative, depth + 1, rows, names, deadline)?;
            } else {
                // The same inode can be named by an unrelated checkout. An
                // object-only census cannot tell which hard-link spelling an
                // FD was opened through, so ambiguity must refuse this offer.
                if metadata.st_nlink != 1 {
                    return Err(invalid());
                }
                // Symlink objects themselves cannot match a followed native
                // cwd/FD target. We never walk out through them.
                if object(&metadata).device != self.identity.device {
                    return Err(invalid());
                }
                #[cfg(target_os = "linux")]
                if mount_at(folder.as_raw_fd(), part, libc::AT_SYMLINK_NOFOLLOW)?
                    != self.identity.mount
                {
                    return Err(invalid());
                }
                rows.push(entry(relative, &metadata));
            }
        }
        if entry(relative.into(), &before) != entry(relative.into(), &stat(folder.as_raw_fd())?) {
            return Err(invalid());
        }
        Ok(())
    }
}

pub(super) struct Inventory {
    entries: Vec<Entry>,
    objects: BTreeSet<Object>,
}
impl Inventory {
    pub(super) fn contains(&self, object: Object) -> bool {
        self.objects.contains(&object)
    }
    pub(super) fn current(&self, scope: &Scope, deadline: Instant) -> io::Result<()> {
        if self.entries != scope.inventory(deadline)?.entries {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn deadline() -> Instant {
        Instant::now() + std::time::Duration::from_secs(3)
    }
    #[test]
    fn only_native_objects_in_the_original_tree_match_even_with_path_aliases() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().canonicalize().unwrap();
        let root = parent.join("work");
        std::fs::create_dir(&root).unwrap();
        let inside = root.join("inside");
        let outside = parent.join("outside");
        std::fs::write(&inside, b"original").unwrap();
        std::fs::write(&outside, b"other").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("alias")).unwrap();
        let held = File::open(&inside).unwrap();
        let scope = Scope::open(&root, deadline()).unwrap();
        let first = scope.inventory(deadline()).unwrap();
        assert!(first.contains(object(&stat(held.as_raw_fd()).unwrap())));
        assert!(!first.contains(object(
            &stat(File::open(root.join("alias")).unwrap().as_raw_fd()).unwrap()
        )));
        // Reading twice must neither share a directory offset nor mutate it.
        first.current(&scope, deadline()).unwrap();
        std::fs::rename(&inside, parent.join("moved-away")).unwrap();
        std::fs::write(&inside, b"replacement").unwrap();
        assert!(first.current(&scope, deadline()).is_err());
        let second = scope.inventory(deadline()).unwrap();
        assert!(!second.contains(object(&stat(held.as_raw_fd()).unwrap())));
    }
    #[test]
    fn changed_root_or_ancestor_and_expired_inventories_refuse() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().canonicalize().unwrap();
        let middle = parent.join("original");
        let root = middle.join("work");
        std::fs::create_dir_all(&root).unwrap();
        let scope = Scope::open(&root, deadline()).unwrap();
        let first = scope.inventory(deadline()).unwrap();
        assert!(scope.inventory(Instant::now()).is_err());
        std::fs::rename(&middle, parent.join("old")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        assert!(scope.current(deadline()).is_err());
        assert!(first.current(&scope, deadline()).is_err());
    }
    #[test]
    fn a_hard_link_from_another_folder_cannot_turn_its_programs_into_targets() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().canonicalize().unwrap();
        let root = parent.join("work");
        std::fs::create_dir(&root).unwrap();
        let outside = parent.join("outside");
        std::fs::write(&outside, b"other checkout").unwrap();
        std::fs::hard_link(&outside, root.join("alias")).unwrap();
        let scope = Scope::open(&root, deadline()).unwrap();
        assert!(scope.inventory(deadline()).is_err());
        std::fs::remove_file(root.join("alias")).unwrap();
        scope.inventory(deadline()).unwrap();
    }
}
