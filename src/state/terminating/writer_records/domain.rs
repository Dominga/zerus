//! Fresh namespace provenance, not writer enrollment or effect authority.
use super::*;
use crate::{platform, state::repositories::directory::Directory};
use std::{fs::File, io::Read, path::PathBuf};

pub(in crate::state) fn creation_record(bytes: &[u8]) -> Result<serde_json::Value> {
    super::marker::source_record(bytes)
}

/// Only the original supervisor's sealed creation source can return this live
/// resource. Stored identities and observations have no conversion into it.
pub(in crate::state) struct OriginalDomain {
    namespace: Namespace,
    scope: Run,
    owner: Owner,
    configured_root: PathBuf,
}
impl OriginalDomain {
    pub(super) fn matches(&self, record: &serde_json::Value) -> Result<()> {
        self.current()?;
        if record["session_id"] != self.scope.session || record["run_id"] != self.scope.run {
            return Err(REFUSED.into());
        }
        Ok(())
    }
    #[cfg(test)]
    pub(in crate::state) fn fixture(root: &std::path::Path) -> Self {
        let scope = Run::new(
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
        let canonical = platform::canonical(root).unwrap();
        let namespace = seed(
            Directory::open(&canonical).unwrap(),
            &scope,
            Instant::now() + std::time::Duration::from_secs(5),
            || Ok(()),
        )
        .unwrap();
        Self {
            namespace,
            scope,
            owner: Owner::current().unwrap(),
            configured_root: root.into(),
        }
    }
    pub(in crate::state) fn create(
        source: &crate::state::supervisor::CreationSource<'_>,
    ) -> Result<Self> {
        source.start()?;
        let (session, run) = source.scope();
        let scope = Run::new(session, run)?;
        let namespace = seed(source.root()?, &scope, source.until(), || source.check())?;
        let value = Self {
            namespace,
            scope,
            owner: Owner::current()?,
            configured_root: source.configured_root().into(),
        };
        value.current()?;
        source.check()?;
        Ok(value)
    }
    pub(in crate::state) fn current(&self) -> Result<()> {
        if Owner::current()? != self.owner
            || platform::canonical(&self.configured_root).map_err(|_| REFUSED)?
                != self.namespace.root.path
        {
            return Err(REFUSED.into());
        }
        self.namespace.current(&self.scope)
    }
    pub(in crate::state) fn observation(&self) -> serde_json::Value {
        serde_json::json!({"version":1,"session":self.scope.session,"run":self.scope.run,
            "coverage":"initialized_not_enrolled","effect_authority":false})
    }
}

#[cfg(test)]
mod tests;

/// The existing ledger format and native namespace, shared by the primitive
/// tests and the sealed original-owner constructor. No Drop rollback exists.
pub(super) struct Namespace {
    root: Directory,
    base: Directory,
    session: Directory,
    run: Directory,
    gate: File,
    lock: File,
}
impl Namespace {
    fn current(&self, scope: &Run) -> Result<()> {
        for directory in [&self.root, &self.base, &self.session, &self.run] {
            directory.verify()?;
            if !directory.private()? {
                return Err(REFUSED.into());
            }
        }
        exact(&self.run, "gate", &self.gate, super::store::GATE)?;
        exact(&self.run, "lock", &self.lock, b"")?;
        let file = self.run.read_private("pending.json")?;
        let mut bytes = Vec::new();
        file.take(BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| REFUSED)?;
        Ledger::decode(&bytes, scope)?;
        Ok(())
    }
}

fn exact(directory: &Directory, name: &str, held: &File, expected: &[u8]) -> Result<()> {
    let named = directory.read_private(name)?;
    if !platform::verify_private_file(held).map_err(|_| REFUSED)?
        || platform::FileIdentity::of(held).map_err(|_| REFUSED)?
            != platform::FileIdentity::of(&named).map_err(|_| REFUSED)?
    {
        return Err(REFUSED.into());
    }
    let mut bytes = Vec::new();
    named
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| REFUSED)?;
    if bytes != expected {
        return Err(REFUSED.into());
    }
    directory.verify()
}

fn present(parent: &Directory, name: &str) -> Result<bool> {
    parent.verify()?;
    let name = std::ffi::CString::new(name).map_err(|_| REFUSED)?;
    let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: one fixed NUL-free leaf and writable native stat storage.
    let rc = unsafe {
        libc::fstatat(
            parent.descriptor(),
            name.as_ptr(),
            info.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(false)
    } else {
        Err(error.to_string())
    }
}

fn directory(
    parent: &Directory,
    name: &str,
    existing: bool,
    check: &impl Fn() -> Result<()>,
) -> Result<Directory> {
    check()?;
    let child = if present(parent, name)? {
        if !existing {
            return Err(REFUSED.into());
        }
        parent.child(name)?
    } else {
        // Directory::create retains its own staged object through atomic
        // no-replace publication. A changed name cannot substitute another.
        let stage = format!(".original-domain-{}", uuid::Uuid::new_v4());
        let child = parent.create(name, &stage)?;
        parent.sync()?;
        child.sync()?;
        child
    };
    if !child.private()? {
        return Err(REFUSED.into());
    }
    child.verify()?;
    check()?;
    Ok(child)
}

pub(super) fn seed(
    root: Directory,
    scope: &Run,
    until: Instant,
    check: impl Fn() -> Result<()>,
) -> Result<Namespace> {
    Run::new(&scope.session, &scope.run)?;
    current(until)?;
    check()?;
    if !root.private()? {
        return Err(REFUSED.into());
    }
    let base = directory(&root, "terminating-writers", true, &check)?;
    let session = directory(&base, &scope.session, true, &check)?;
    let run = directory(&session, &scope.run, false, &check)?;
    let ledger = Ledger {
        version: VERSION,
        scope: scope.clone(),
        pending: Vec::new(),
    }
    .encode()?;
    let mut created = Vec::new();
    for (name, bytes) in [
        ("gate", super::store::GATE),
        ("lock", &[][..]),
        ("pending.json", ledger.as_slice()),
    ] {
        current(until)?;
        check()?;
        let file = run.write_new_file(name, bytes)?;
        exact(&run, name, &file, bytes)?;
        current(until)?;
        check()?;
        created.push(file);
    }
    let pending = created.pop().ok_or(REFUSED)?;
    let lock = created.pop().ok_or(REFUSED)?;
    let gate = created.pop().ok_or(REFUSED)?;
    run.sync()?;
    session.sync()?;
    base.sync()?;
    root.sync()?;
    // The initial ledger is still this call's exact new object. After this
    // readback it may be replaced by enrolled writers; no immutable data-file
    // identity is claimed for those future legitimate atomic updates.
    exact(&run, "pending.json", &pending, &ledger)?;
    let value = Namespace {
        root,
        base,
        session,
        run,
        gate,
        lock,
    };
    value.current(scope)?;
    check()?;
    current(until)?;
    Ok(value)
}
