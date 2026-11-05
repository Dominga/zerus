//! Native object validation for the closed holder protocol. This layer checks
//! custody DATA only. Native peer/phase admission and stop consent are separate.
use super::*;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use zerus_client_core::worktree_programs::{DirectoryId, Manifest};

pub(crate) mod transport;
#[cfg(target_os = "linux")]
pub(crate) mod peer;
#[cfg(target_os = "linux")]
pub(crate) mod holder;

pub(crate) const MAX_FDS: usize = MAX_DEPTH + 1;
pub(crate) const MAX_FRAME: usize = 64 << 10;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Parent {
    name: Vec<u8>,
    parent: Object,
    child: Object,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Description {
    path: String,
    root: DirectoryId,
    parents: Vec<Parent>,
}

impl Scope {
    /// Borrowed descriptors stay owned by the original holder through sendmsg.
    /// Sending rights duplicates them in the receiver, not in a pathname walk.
    pub(crate) fn native_descriptors(&self) -> Vec<BorrowedFd<'_>> {
        self.ancestors
            .iter()
            .map(|(parent, _, _)| parent.as_fd())
            .chain(std::iter::once(self.root.as_fd()))
            .collect()
    }
    pub(crate) fn describe_loan(&self, deadline: Instant) -> io::Result<Vec<u8>> {
        self.current(deadline)?;
        let parents = self
            .ancestors
            .iter()
            .map(|(fd, name, child)| {
                Ok(Parent {
                    name: name.to_bytes().to_vec(),
                    parent: object(&stat(fd.as_raw_fd())?),
                    child: *child,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let value = Description {
            path: self.path.to_str().ok_or_else(invalid)?.into(),
            root: self.identity.clone(),
            parents,
        };
        let bytes = serde_json::to_vec(&value).map_err(io::Error::other)?;
        if bytes.len() > MAX_FRAME {
            return Err(invalid());
        }
        Ok(bytes)
    }
}

fn received(fd: OwnedFd) -> io::Result<File> {
    let file = File::from(fd);
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
    let access = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || flags & libc::FD_CLOEXEC == 0
        || access < 0
        || access & libc::O_ACCMODE != libc::O_RDONLY
        || stat(file.as_raw_fd())?.st_mode & libc::S_IFMT != libc::S_IFDIR
    {
        return Err(invalid());
    }
    Ok(file)
}

/// Call only after the closed protocol has verified the ORIGINAL native peer,
/// pipeline, manifest, phase and request binding. These comparisons cannot
/// establish that admission and never manufacture a stop/Force permission.
pub(crate) fn receive(
    bytes: &[u8],
    descriptors: Vec<OwnedFd>,
    original: &Manifest,
    deadline: Instant,
) -> io::Result<Scope> {
    check(deadline)?;
    if bytes.len() > MAX_FRAME || !(2..=MAX_FDS).contains(&descriptors.len()) {
        return Err(invalid());
    }
    let description: Description = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    if description.path != original.capture.root
        || description.root != original.checkout
        || description.parents.len() + 1 != descriptors.len()
    {
        return Err(invalid());
    }
    let parts = Path::new(&description.path)
        .components()
        .filter_map(|component| match component {
            Component::RootDir => None,
            Component::Normal(part) => Some(Ok(part.as_bytes())),
            _ => Some(Err(invalid())),
        })
        .collect::<io::Result<Vec<_>>>()?;
    if parts.len() != description.parents.len()
        || description
            .parents
            .iter()
            .zip(parts)
            .any(|(parent, part)| parent.name != part)
    {
        return Err(invalid());
    }
    let mut files = descriptors
        .into_iter()
        .map(received)
        .collect::<io::Result<Vec<_>>>()?;
    let root = files.pop().ok_or_else(invalid)?;
    let root_id = object(&stat(root.as_raw_fd())?);
    if (root_id.device, root_id.inode) != (description.root.device, description.root.inode)
        || mount(root.as_raw_fd())? != description.root.mount
    {
        return Err(invalid());
    }
    let ancestors = files
        .into_iter()
        .zip(description.parents)
        .map(|(file, parent)| {
            if object(&stat(file.as_raw_fd())?) != parent.parent {
                return Err(invalid());
            }
            Ok((file, CString::new(parent.name)?, parent.child))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let scope = Scope {
        path: description.path.into(),
        ancestors,
        root,
        identity: description.root,
    };
    scope.current(deadline)?;
    Ok(scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn until() -> Instant {
        Instant::now() + std::time::Duration::from_secs(3)
    }
    fn copies(scope: &Scope) -> Vec<OwnedFd> {
        scope
            .native_descriptors()
            .iter()
            .map(|fd| {
                let copy = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
                assert!(copy >= 0);
                unsafe { OwnedFd::from_raw_fd(copy) }
            })
            .collect()
    }
    fn manifest(scope: &Scope) -> Manifest {
        let id = "11111111-1111-4111-8111-111111111111";
        Manifest {
            abi: zerus_client_core::worktree_programs::ABI.into(),
            id: id.into(),
            capture: zerus_client_core::termination_cleanup::Capture {
                target: zerus_client_core::Target {
                    computer: String::new(),
                    name: "codex/example/original".into(),
                    run_id: id.into(),
                    conversation_id: id.into(),
                },
                machine: id.into(),
                owner_id: id.into(),
                root: scope.path.to_str().unwrap().into(),
                common_dir: "/srv/example/main/.git".into(),
            },
            configuration: "a".repeat(64),
            boot: id.into(),
            checkout: scope.identity.clone(),
            issued: 1000,
            expires: 1300,
            programs: Vec::new(),
        }
    }
    #[test]
    fn the_received_scope_keeps_the_original_objects_after_the_senders_handles_close() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let scope = Scope::open(&root, until()).unwrap();
        let original = manifest(&scope);
        let bytes = scope.describe_loan(until()).unwrap();
        let fds = copies(&scope);
        drop(scope);
        let loan = receive(&bytes, fds, &original, until()).unwrap();
        loan.current(until()).unwrap();
        assert_eq!(loan.identity(), &original.checkout);
        assert!(loan
            .native_descriptors()
            .iter()
            .all(
                |fd| unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC != 0
            ));
    }
    #[test]
    fn received_names_objects_counts_and_close_on_exec_cannot_be_substituted() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let scope = Scope::open(&root, until()).unwrap();
        let original = manifest(&scope);
        let bytes = scope.describe_loan(until()).unwrap();
        let mut wrong = original.clone();
        wrong.checkout.inode += 1;
        assert!(receive(&bytes, copies(&scope), &wrong, until()).is_err());
        let mut raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        raw["path"] = "/srv/example/substitute".into();
        assert!(receive(
            &serde_json::to_vec(&raw).unwrap(),
            copies(&scope),
            &original,
            until()
        )
        .is_err());
        let mut short = copies(&scope);
        short.pop();
        assert!(receive(&bytes, short, &original, until()).is_err());
        let flags = copies(&scope);
        assert_eq!(
            unsafe { libc::fcntl(flags[0].as_raw_fd(), libc::F_SETFD, 0) },
            0
        );
        assert!(receive(&bytes, flags, &original, until()).is_err());
        let other = tempfile::tempdir().unwrap();
        let mut substituted = copies(&scope);
        substituted.pop();
        substituted.push(File::open(other.path()).unwrap().into());
        assert!(receive(&bytes, substituted, &original, until()).is_err());
        assert!(receive(
            &vec![b' '; MAX_FRAME + 1],
            copies(&scope),
            &original,
            until()
        )
        .is_err());
        assert!(receive(&bytes, copies(&scope), &original, Instant::now()).is_err());
    }
}
