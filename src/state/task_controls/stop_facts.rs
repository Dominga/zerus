//! A separate durable outbox: rollback writers may rebuild the run record.
//! No desktop database or callback is reachable while this publisher holds
//! the native writer. A detached bounded snapshot is imported after release.
use crate::{platform, state::repositories::directory::Directory, state::storage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};
use zerus_desktop_ledger::task_controls::{Executor, EXECUTOR_HOLDS, TASK_CONTROLS};

#[cfg(windows)]
#[path = "stop_facts/coexistence.rs"]
mod coexistence;

type Result<T> = std::result::Result<T, String>;
const BYTES: usize = 256 << 10;

/// Inactive selected support. Between leases this owns only original data
/// custody; it has no current facts getter or actor/proof conversion.
#[cfg(windows)]
pub(in crate::state) struct SelectedCapture(coexistence::Seal);

/// Retain this exact capability for the whole logical read and final check.
/// Returned copies remain data; transaction users retain this through COMMIT.
#[cfg(windows)]
pub(in crate::state) struct SelectedLease<'a>(coexistence::CurrentLease<'a>);

#[cfg(windows)]
#[must_use = "Failed original custody must be explicitly retired"]
pub(in crate::state) struct CaptureFailed(coexistence::Failed);

#[cfg(windows)]
impl std::fmt::Debug for CaptureFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureFailed")
            .field("stage", &self.0.stage())
            .field("reason", &self.0.detail())
            .finish()
    }
}

#[cfg(windows)]
impl CaptureFailed {
    pub(in crate::state) fn retire(self) {
        self.0.retire();
    }
}

#[cfg(windows)]
impl SelectedCapture {
    pub(in crate::state) fn lease(
        &self,
        executor: &Executor,
    ) -> std::result::Result<SelectedLease<'_>, &'static str> {
        self.0
            .lease(executor)
            .map(SelectedLease)
            .map_err(|_| "The selected Stop capture no longer matches its current scope")
    }

    pub(in crate::state) fn retire(self) {
        self.0.retire();
    }
}

#[cfg(windows)]
impl SelectedLease<'_> {
    pub(in crate::state) fn read_scoped<T>(
        &self,
        read: impl FnOnce(&Executor, &[Fact]) -> T,
    ) -> std::result::Result<T, &'static str> {
        self.0
            .read_scoped(read)
            .map_err(|_| "The selected Stop lease refused its complete read or final check")
    }

    pub(in crate::state) fn final_check(&self) -> std::result::Result<(), &'static str> {
        self.0
            .final_check()
            .map_err(|_| "The selected Stop lease refused its final check")
    }
}

#[cfg(windows)]
pub(super) fn capture_owned(
    owned: super::writer::Writer,
    executor: &Executor,
) -> std::result::Result<SelectedCapture, CaptureFailed> {
    coexistence::from_owned(owned, executor.clone())
        .map(SelectedCapture)
        .map_err(CaptureFailed)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Source {
    PersonStop,
    VerifiedExternalInterrupt,
}

/// Dispatch-held evidence only, never physical process/job completion.
/// A normal provider Stop hook cannot be represented as an interrupt source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fact {
    pub id: String,
    pub executor: Executor,
    pub source: Source,
    pub turn: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Overflow {
    id: String,
    original: Fact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    schema: u32,
    executor: Executor,
    pub facts: Vec<Fact>,
    overflow: Option<Overflow>,
}
impl Snapshot {
    pub fn held_unknown(&self) -> bool {
        self.overflow.is_some()
    }

    fn validate(&self, executor: &Executor) -> Result<()> {
        if self.schema != 1
            || self.executor.key().map_err(err)? != executor.key().map_err(err)?
            || self.executor.session_id != executor.session_id
            || self.facts.len() > EXECUTOR_HOLDS
        {
            return Err("The original Stop fact scope or bound changed; review required".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for fact in self
            .facts
            .iter()
            .chain(self.overflow.iter().map(|o| &o.original))
        {
            fact.validate()?;
            if fact.executor.key().map_err(err)? != executor.key().map_err(err)?
                || fact.executor.session_id != executor.session_id
                || !ids.insert(&fact.id)
            {
                return Err("The original Stop fact identity changed; review required".into());
            }
        }
        if self
            .overflow
            .as_ref()
            .is_some_and(|o| o.id != o.original.id || self.facts.len() != EXECUTOR_HOLDS)
        {
            return Err("Invalid Stop fact overflow; review required".into());
        }
        Ok(())
    }
}
impl Fact {
    fn validate(&self) -> Result<()> {
        self.executor.validate().map_err(err)?;
        if !uuid::Uuid::parse_str(&self.id)
            .is_ok_and(|id| id.to_string() == self.id && !id.is_nil())
            || self
                .turn
                .as_ref()
                .is_some_and(|t| t.is_empty() || t.len() > 512 || t.chars().any(char::is_control))
        {
            return Err("Invalid original Stop fact identity".into());
        }
        Ok(())
    }
}

pub(super) struct Publisher<'a> {
    root: Directory,
    directory: Directory,
    writer: &'a storage::Guard,
    executor: Executor,
    name: String,
}

/// Gate-off performs no filesystem access. `new_run` is supplied only by the
/// original verified launch/admission; a missing known-run file must refuse.
pub(super) fn open<'a>(
    root: &Path,
    writer: &'a storage::Guard,
    executor: &Executor,
    new_run: bool,
) -> Result<Option<Publisher<'a>>> {
    if !TASK_CONTROLS {
        return Ok(None);
    }
    Publisher::open_enabled(root, writer, executor, new_run).map(Some)
}

impl<'a> Publisher<'a> {
    fn open_enabled(
        root: &Path,
        writer: &'a storage::Guard,
        executor: &Executor,
        new_run: bool,
    ) -> Result<Self> {
        executor.validate().map_err(err)?;
        let root = Directory::open(root)?;
        if !root.private()? {
            return Err("The native state directory is not private".into());
        }
        verify_writer(&root, writer)?;
        let directory = if new_run {
            root.ensure_child("task-stop-facts")?
        } else {
            root.child("task-stop-facts")?
        };
        if !directory.private()? {
            return Err("The Stop fact directory is not private".into());
        }
        let publisher = Self {
            name: format!("{}.json", executor.key().map_err(err)?),
            root,
            directory,
            writer,
            executor: executor.clone(),
        };
        // No missing-file inference during ordinary admission or restart.
        match std::fs::symlink_metadata(publisher.directory.path.join(&publisher.name)) {
            Err(e) if new_run && e.kind() == std::io::ErrorKind::NotFound => {
                let initial = Snapshot {
                    schema: 1,
                    executor: executor.clone(),
                    facts: vec![],
                    overflow: None,
                };
                publisher
                    .directory
                    .write_new(&publisher.name, &serde_json::to_vec(&initial).map_err(err)?)?;
            }
            Err(e) => return Err(err(e)),
            Ok(_) => (),
        }
        publisher.snapshot()?;
        Ok(publisher)
    }

    fn verify(&self) -> Result<()> {
        self.root.verify()?;
        self.directory.verify()?;
        if !self.root.private()? || !self.directory.private()? {
            return Err("The original Stop fact directory changed".into());
        }
        verify_writer(&self.root, self.writer)
    }

    fn read(&self) -> Result<(Snapshot, platform::FileIdentity, Vec<u8>, File)> {
        self.verify()?;
        let file = self.directory.read_private(&self.name)?;
        let identity = platform::FileIdentity::of(&file).map_err(err)?;
        let mut bytes = Vec::new();
        (&file)
            .take(BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() > BYTES {
            return Err("Stop facts exceed the bound; review required".into());
        }
        let snapshot: Snapshot = serde_json::from_slice(&bytes).map_err(err)?;
        snapshot.validate(&self.executor)?;
        self.verify()?;
        let current = self.directory.read_private(&self.name)?;
        if platform::FileIdentity::of(&current).map_err(err)? != identity {
            return Err("Stop facts changed during capture; review required".into());
        }
        Ok((snapshot, identity, bytes, file))
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        self.read().map(|(s, _, _, _)| s)
    }

    /// Consume the publisher but not its caller's native lock. The caller
    /// drops that lock before passing this sealed read-only evidence to the
    /// service actor. Its retained file is rechecked inside the DB transaction.
    pub fn capture(self) -> Result<Captured> {
        let (snapshot, identity, bytes, fact_file) = self.read()?;
        let writer = self.writer.identity().map_err(err)?;
        // Open independently: this pins the original lock-file object, never
        // duplicates or retains the caller's native exclusion/lock handle.
        let writer_file = self.root.read_private(".lock")?;
        if platform::FileIdentity::of(&writer_file).map_err(err)? != writer {
            return Err("The original native writer changed during capture".into());
        }
        Ok(Captured {
            root: self.root,
            directory: self.directory,
            name: self.name,
            snapshot,
            identity,
            writer,
            fact_file,
            writer_file,
            bytes: bytes.len(),
            digest: Sha256::digest(&bytes).into(),
        })
    }

    /// Called with the original native writer, before an explicit Stop effect
    /// or while recording a verified external interrupt. Never clears facts.
    pub fn publish(&self, fact: &Fact) -> Result<Snapshot> {
        fact.validate()?;
        if fact.executor.key().map_err(err)? != self.executor.key().map_err(err)?
            || fact.executor.session_id != self.executor.session_id
        {
            return Err("The Stop fact belongs to another run/birth/child".into());
        }
        let (mut snapshot, identity, _, _) = self.read()?;
        if let Some(old) = snapshot.facts.iter().find(|f| f.id == fact.id).or_else(|| {
            snapshot
                .overflow
                .as_ref()
                .map(|o| &o.original)
                .filter(|f| f.id == fact.id)
        }) {
            return if old == fact {
                Ok(snapshot)
            } else {
                Err("Stop UUID payload changed".into())
            };
        }
        if snapshot.overflow.is_some() {
            return Ok(snapshot);
        } // Already durably Unknown/held.
        if snapshot.facts.len() == EXECUTOR_HOLDS {
            snapshot.overflow = Some(Overflow {
                id: fact.id.clone(),
                original: fact.clone(),
            });
        } else {
            snapshot.facts.push(fact.clone());
        }
        snapshot.validate(&self.executor)?;
        let bytes = serde_json::to_vec(&snapshot).map_err(err)?;
        if bytes.len() > BYTES {
            return Err("Stop facts exceed the bound; review required".into());
        }
        self.verify()?;
        let current = self.directory.read_private(&self.name)?;
        if platform::FileIdentity::of(&current).map_err(err)? != identity {
            return Err("Stop facts changed before publication; review required".into());
        }
        storage::cut_write()?;
        self.directory.replace_file(&self.name, &bytes)?;
        self.verify()?;
        Ok(snapshot)
    }
}

/// No Deserialize/Clone/public constructor: only the original native writer
/// can capture this bounded evidence. Unlocked original files prevent numeric
/// identities from recycling until import settles. It cannot authorize
/// input or a Person. A failed recheck must block admission, never be skipped.
pub(crate) struct Captured {
    root: Directory,
    directory: Directory,
    name: String,
    snapshot: Snapshot,
    identity: platform::FileIdentity,
    writer: platform::FileIdentity,
    fact_file: File,
    writer_file: File,
    bytes: usize,
    digest: [u8; 32],
}
impl Captured {
    pub fn executor(&self) -> &Executor {
        &self.snapshot.executor
    }
    pub fn facts(&self) -> &[Fact] {
        &self.snapshot.facts
    }
    /// Bounded no-follow read only: never takes the native writer under a DB
    /// transaction. Even equal bytes in a replaced object require review.
    pub fn verify(&self) -> Result<()> {
        self.root.verify()?;
        self.directory.verify()?;
        if !self.root.private()? || !self.directory.private()? {
            return Err("The original Stop fact directories changed".into());
        }
        let lock = self.root.read_private(".lock")?;
        let file = self.directory.read_private(&self.name)?;
        if platform::FileIdentity::of(&self.writer_file).map_err(err)? != self.writer
            || platform::FileIdentity::of(&self.fact_file).map_err(err)? != self.identity
            || platform::FileIdentity::of(&lock).map_err(err)? != self.writer
            || platform::FileIdentity::of(&file).map_err(err)? != self.identity
        {
            return Err("The captured Stop fact or original writer changed".into());
        }
        let mut bytes = Vec::new();
        (&file)
            .take(BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() > BYTES
            || bytes.len() != self.bytes
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != self.digest
        {
            return Err("The captured Stop fact bytes changed; admission held for review".into());
        }
        let current = self.directory.read_private(&self.name)?;
        if platform::FileIdentity::of(&current).map_err(err)? != self.identity {
            return Err("The captured Stop fact was replaced; admission held for review".into());
        }
        self.root.verify()?;
        self.directory.verify()?;
        if self.snapshot.held_unknown() {
            return Err("Stop fact overflow; admission held for explicit review".into());
        }
        Ok(())
    }
}

pub(super) fn verify_writer(root: &Directory, writer: &storage::Guard) -> Result<()> {
    let file = root.read_private(".lock")?;
    if platform::FileIdentity::of(&file).map_err(err)? != writer.identity().map_err(err)? {
        return Err("The original native state writer changed".into());
    }
    Ok(())
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests;

/// Private fixture constructor through the actual platform capture path.
/// Its returned path is test DATA, never a namespace or actor capability.
#[cfg(test)]
pub(super) fn fixture_capture(
    root: &Path,
    fact: &Fact,
    until: std::time::Instant,
) -> Result<(super::Capture, std::path::PathBuf)> {
    let owned = super::writer::test_open(root, until)?;
    let publisher = Publisher::open_enabled(owned.root()?, owned.guard()?, &fact.executor, true)?;
    publisher.publish(fact)?;
    let path = publisher.directory.path.join(&publisher.name);
    #[cfg(windows)]
    {
        drop(publisher);
        match super::capture_owned_stops(owned, &fact.executor) {
            Ok(captured) => Ok((super::Capture::owned(captured), path)),
            Err(failed) => {
                let reason = format!("fixture capture refused: {failed:?}");
                failed.retire();
                Err(reason)
            }
        }
    }
    #[cfg(not(windows))]
    {
        let captured = publisher.capture()?;
        owned.verify()?;
        drop(owned);
        Ok((super::Capture::legacy(captured), path))
    }
}
