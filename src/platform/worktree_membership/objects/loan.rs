//! Native object validation for the closed holder protocol. This layer checks
//! custody DATA only. Native peer/phase admission and stop consent are separate.
use super::*;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use zerus_client_core::worktree_programs::custody::{clock, Binding};
use zerus_client_core::worktree_programs::{DirectoryId, Manifest};

pub(crate) mod borrower;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod holder;
#[cfg(target_os = "linux")]
pub(crate) mod peer;
#[cfg(target_os = "macos")]
#[path = "loan/peer_macos.rs"]
pub(crate) mod peer;
pub(crate) mod transport;

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

/// A host-local DATA transfer, never a grant. Bind the delivered objects to
/// the exact original request/phase and the SAME absolute operation deadline.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Transfer {
    version: u32,
    binding: Binding,
    deadline_millis: u64,
    objects: Description,
    // Actual original sender-owned Scope descriptor, not a caller-selected
    // exemption. Native peer, transferred objects and flags remain independent.
    holder_root_descriptor: i32,
}

pub(crate) fn transfer(
    scope: &Scope,
    binding: &Binding,
    deadline_millis: u64,
    until: Instant,
) -> io::Result<Vec<u8>> {
    if !binding.valid() {
        return Err(invalid());
    }
    let deadline = until.min(clock::restore(deadline_millis)?);
    let objects =
        serde_json::from_slice(&scope.describe_loan(deadline)?).map_err(io::Error::other)?;
    let bytes = serde_json::to_vec(&Transfer {
        version: 1,
        binding: binding.clone(),
        deadline_millis,
        objects,
        holder_root_descriptor: scope.root.as_raw_fd(),
    })
    .map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME {
        return Err(invalid());
    }
    check(deadline)?;
    Ok(bytes)
}

pub(crate) fn receive_transfer(
    bytes: &[u8],
    descriptors: Vec<OwnedFd>,
    binding: &Binding,
    original: &Manifest,
    until: Instant,
) -> io::Result<(Scope, Instant, i32)> {
    if bytes.len() > MAX_FRAME || !binding.valid() {
        return Err(invalid());
    }
    let transfer: Transfer = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    if transfer.version != 1 || transfer.binding != *binding || transfer.holder_root_descriptor < 3
    {
        return Err(invalid());
    }
    let deadline = until.min(clock::restore(transfer.deadline_millis)?);
    let objects = serde_json::to_vec(&transfer.objects).map_err(io::Error::other)?;
    let scope = receive(&objects, descriptors, original, deadline)?;
    check(deadline)?;
    Ok((scope, deadline, transfer.holder_root_descriptor))
}

impl Scope {
    pub(in crate::platform::worktree_membership) fn own_root_descriptor(&self) -> BorrowedFd<'_> {
        self.root.as_fd()
    }
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
    let filesystem_root = directory(libc::AT_FDCWD, c"/")?;
    if description.parents.first().map(|parent| parent.parent)
        != Some(object(&stat(filesystem_root.as_raw_fd())?))
        || description
            .parents
            .windows(2)
            .any(|pair| pair[0].child != pair[1].parent)
        || description.parents.last().map(|parent| parent.child) != Some(root_id)
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
    fn transfers_bind_the_exact_phase_and_cannot_restart_an_absolute_deadline() {
        let temporary = tempfile::tempdir().unwrap();
        let scope = Scope::open(&temporary.path().canonicalize().unwrap(), until()).unwrap();
        let original = manifest(&scope);
        let binding = Binding {
            pipeline: uuid::Uuid::new_v4().to_string(),
            manifest: "a".repeat(64),
            request: uuid::Uuid::new_v4().to_string(),
            input: "b".repeat(64),
        };
        let bound = until();
        let stamp = clock::freeze(bound).unwrap();
        let bytes = transfer(&scope, &binding, stamp, bound).unwrap();
        let (received, deadline, descriptor) =
            receive_transfer(&bytes, copies(&scope), &binding, &original, until()).unwrap();
        assert!(deadline <= bound);
        assert_eq!(descriptor, scope.root.as_raw_fd());
        received.current(deadline).unwrap();
        for change in 0..8 {
            let mut packet: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            match change {
                0 => packet["binding"]["request"] = uuid::Uuid::new_v4().to_string().into(),
                1 => packet["binding"]["pipeline"] = uuid::Uuid::new_v4().to_string().into(),
                2 => packet["binding"]["input"] = "c".repeat(64).into(),
                3 => packet["deadline_millis"] = clock::now().unwrap().saturating_sub(1).into(),
                4 => packet["deadline_millis"] = u64::MAX.into(),
                5 => packet["version"] = 2.into(),
                6 => packet["holder_root_descriptor"] = 0.into(),
                _ => packet["stop"] = true.into(),
            }
            assert!(
                receive_transfer(
                    &serde_json::to_vec(&packet).unwrap(),
                    copies(&scope),
                    &binding,
                    &original,
                    until()
                )
                .is_err(),
                "change {change}"
            );
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
    fn a_received_chain_must_start_at_the_native_filesystem_root() {
        let temporary = tempfile::tempdir().unwrap();
        let native_root = temporary.path().canonicalize().unwrap();
        let checkout = native_root.join("first/second/checkout");
        std::fs::create_dir_all(&checkout).unwrap();
        let mut scope = Scope::open(&checkout, until()).unwrap();
        // Every individual edge is genuine, but the claimed absolute spelling
        // starts at a private directory instead of the native filesystem root.
        scope.path = "/first/second/checkout".into();
        let keep = scope.ancestors.len() - 3;
        scope.ancestors.drain(..keep);
        let original = manifest(&scope);
        let bytes = scope.describe_loan(until()).unwrap();
        assert!(receive(&bytes, copies(&scope), &original, until()).is_err());
    }
    #[test]
    fn individually_real_ancestor_edges_cannot_replace_a_connected_chain() {
        let temporary = tempfile::tempdir().unwrap();
        let checkout = temporary.path().join("first/second/checkout");
        std::fs::create_dir_all(&checkout).unwrap();
        let mut scope = Scope::open(&checkout.canonicalize().unwrap(), until()).unwrap();
        let other = tempfile::tempdir().unwrap();
        let (parent, part, child) = &mut scope.ancestors[1];
        let name = OsStr::from_bytes(part.to_bytes());
        let detached = other.path().join(name);
        std::fs::create_dir(&detached).unwrap();
        *parent = File::open(other.path()).unwrap();
        *child = object(&stat(File::open(&detached).unwrap().as_raw_fd()).unwrap());
        let original = manifest(&scope);
        // describe/current still verify every real child independently; the
        // receiver must additionally link each child to the next parent.
        let bytes = scope.describe_loan(until()).unwrap();
        assert!(receive(&bytes, copies(&scope), &original, until()).is_err());
    }
    #[test]
    fn the_last_ancestor_child_must_be_the_original_received_checkout() {
        let temporary = tempfile::tempdir().unwrap();
        let checkout = temporary.path().join("first/second/checkout");
        std::fs::create_dir_all(&checkout).unwrap();
        let mut scope = Scope::open(&checkout.canonicalize().unwrap(), until()).unwrap();
        let other = tempfile::tempdir().unwrap();
        let replacement = Scope::open(&other.path().canonicalize().unwrap(), until()).unwrap();
        // The original checkout ID and received root agree, and the complete
        // native ancestor chain agrees internally, but it leads elsewhere.
        scope.root = replacement.root;
        scope.identity = replacement.identity;
        let original = manifest(&scope);
        let bytes = scope.describe_loan(until()).unwrap();
        assert!(receive(&bytes, copies(&scope), &original, until()).is_err());
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
