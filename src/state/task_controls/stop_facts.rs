//! A separate durable outbox: rollback writers may rebuild the run record.
//! No desktop database or callback is reachable while this publisher holds
//! the native writer. A detached bounded snapshot is imported after release.
use crate::{platform, state::repositories::directory::Directory, state::storage};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};
use zerus_desktop_ledger::task_controls::{Executor, EXECUTOR_HOLDS, TASK_CONTROLS};

type Result<T> = std::result::Result<T, String>;
const BYTES: usize = 256 << 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Source {
    PersonStop,
    VerifiedExternalInterrupt,
}

/// Dispatch-held evidence only, never physical process/job completion.
/// A normal provider Stop hook cannot be represented as an interrupt source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Fact {
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

    fn read(&self) -> Result<(Snapshot, platform::FileIdentity)> {
        self.verify()?;
        let file = self.directory.read_private(&self.name)?;
        let identity = platform::FileIdentity::of(&file).map_err(err)?;
        let mut bytes = Vec::new();
        file.take(BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() > BYTES {
            return Err("Stop facts exceed the bound; review required".into());
        }
        let snapshot: Snapshot = serde_json::from_slice(&bytes).map_err(err)?;
        snapshot.validate(&self.executor)?;
        self.verify()?;
        Ok((snapshot, identity))
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        self.read().map(|(s, _)| s)
    }

    /// Called with the original native writer, before an explicit Stop effect
    /// or while recording a verified external interrupt. Never clears facts.
    pub fn publish(&self, fact: &Fact) -> Result<Snapshot> {
        fact.validate()?;
        if fact.executor.key().map_err(err)? != self.executor.key().map_err(err)? {
            return Err("The Stop fact belongs to another run/birth/child".into());
        }
        let (mut snapshot, identity) = self.read()?;
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

fn verify_writer(root: &Directory, writer: &storage::Guard) -> Result<()> {
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
