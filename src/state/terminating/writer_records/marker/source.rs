use super::*;
use crate::{
    platform,
    state::{
        self,
        guarded_recovery::{Effects, Original as ReadSource, View},
        repositories::directory::Directory,
        storage,
    },
};
use std::{
    cell::{Cell, RefCell},
    fs::File,
    path::{Path, PathBuf},
};

pub(super) struct Source {
    store: super::super::Store,
    scope: super::super::Run,
    state_lock: File,
    root: Arc<Directory>,
    configured_root: PathBuf,
    config: Arc<Directory>,
    configured_config: PathBuf,
    path: PathBuf,
    binding: Value,
    owner: super::super::Owner,
    until: Instant,
}
fn config() -> PathBuf {
    crate::config::config_dir(&state::home().to_string_lossy())
}

impl Source {
    #[cfg(test)]
    pub(super) fn fixture_deadline(&mut self, until: Instant) {
        self.until = until;
    }
    #[cfg(test)]
    pub(super) fn fixture_wrong_owner(&mut self) {
        self.owner.pid = 0;
    }
    #[cfg(test)]
    pub(super) fn capture(writer: &storage::Guard, record: &Value, until: Instant) -> Result<Self> {
        let binding = codec::binding(record)?;
        let scope = super::super::Run::new(
            codec::text(record, "session_id", 36)?,
            codec::text(record, "run_id", 36)?,
        )?;
        let store = super::super::Store::open(&scope, until)?;
        let state_lock = store.state_lock()?;
        store.check_writer(&state_lock, writer)?;
        let configured_root = storage::root();
        let configured_config = config();
        if !configured_root.is_absolute() || !configured_config.is_absolute() {
            return Err(super::super::REFUSED.into());
        }
        let root = Arc::new(Directory::open(
            &platform::canonical(&configured_root).map_err(|_| super::super::REFUSED)?,
        )?);
        let config = Arc::new(Directory::open(
            &platform::canonical(&configured_config).map_err(|_| super::super::REFUSED)?,
        )?);
        let path = storage::own_path(record)?;
        let relative = path
            .strip_prefix(&configured_root)
            .map_err(|_| super::super::REFUSED)?;
        if relative.components().count() > 2 {
            return Err(super::super::REFUSED.into());
        }
        let path = root.path.join(relative);
        let value = Self {
            store,
            scope,
            state_lock,
            root,
            configured_root,
            config,
            configured_config,
            path,
            binding,
            owner: super::super::Owner::current()?,
            until,
        };
        value.current_record(None)?;
        Ok(value)
    }
    pub(super) fn scope(&self) -> &super::super::Run {
        &self.scope
    }
    pub(super) fn binding(&self) -> &Value {
        &self.binding
    }
    pub(super) fn owner(&self) -> &super::super::Owner {
        &self.owner
    }
    pub(super) fn check(&self) -> Result<()> {
        super::super::current(self.until)?;
        if super::super::Owner::current()? != self.owner
            || storage::root() != self.configured_root
            || config() != self.configured_config
            || platform::canonical(&self.configured_root).map_err(|_| super::super::REFUSED)?
                != self.root.path
            || platform::canonical(&self.configured_config).map_err(|_| super::super::REFUSED)?
                != self.config.path
            || !self.root.private()?
            || !self.config.private()?
        {
            return Err(super::super::REFUSED.into());
        }
        self.root.verify()?;
        self.config.verify()?;
        super::super::current(self.until)
    }
    fn selected(&self, path: &Path) -> Result<PathBuf> {
        self.check()?;
        let relative = path
            .strip_prefix(&self.root.path)
            .or_else(|_| path.strip_prefix(&self.configured_root))
            .map_err(|_| super::super::REFUSED)?;
        let selected = self.root.path.join(relative);
        if !path.is_absolute()
            || relative.components().count() > 2
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            || selected == self.root.path.join(".lock")
        {
            return Err(super::super::REFUSED.into());
        }
        Ok(selected)
    }
    fn read(&self, path: &Path, limit: u64) -> Result<Option<ReadSource>> {
        let path = self.selected(path)?;
        let value = ReadSource::read_existing(&path, limit.min(RECORD_BYTES as u64))?;
        if let Some(file) = value.as_ref().and_then(ReadSource::file) {
            if !platform::verify_private_file(file).map_err(|_| super::super::REFUSED)? {
                return Err(super::super::REFUSED.into());
            }
        }
        self.check()?;
        Ok(value)
    }
    fn current_record(&self, own: Option<&Data>) -> Result<Value> {
        let original = self
            .read(&self.path, RECORD_BYTES as u64)?
            .ok_or(super::super::REFUSED)?;
        let record = codec::record(original.bytes())?;
        // Native reads retain the canonical original directory. The normal
        // metadata validator compares the configured layout route as data;
        // it does not open it. check() pins that alias before/after this read.
        let route = self.configured_root.join(
            self.path
                .strip_prefix(&self.root.path)
                .map_err(|_| super::super::REFUSED)?,
        );
        storage::verify_metadata_at(
            &route,
            codec::text(&self.binding, "name", 512)?,
            original.bytes(),
        )?;
        let mut projected = record.clone();
        match own {
            None if record.get("terminating").is_some() => return Err(super::super::REFUSED.into()),
            Some(data)
                if record.get("terminating")
                    != Some(&serde_json::to_value(data).map_err(|_| super::super::REFUSED)?) =>
            {
                return Err(super::super::REFUSED.into());
            }
            _ => {}
        }
        projected
            .as_object_mut()
            .ok_or(super::super::REFUSED)?
            .remove("terminating");
        if codec::binding(&projected)? != self.binding {
            return Err(super::super::REFUSED.into());
        }
        original.verify()?;
        self.check()?;
        Ok(record)
    }
    pub(super) fn publish(&self, gate: &RunWriterGate, data: &Data, until: Instant) -> Result<()> {
        self.check()?;
        self.store.check_writer(&self.state_lock, gate.writer()?)?;
        let mut record = self.current_record(None)?;
        record["terminating"] = serde_json::to_value(data).map_err(|_| super::super::REFUSED)?;
        codec::encoded(&record, RECORD_BYTES)?;
        let effects = Publication {
            source: self,
            gate,
            data,
            until,
            committed: Cell::new(false),
            failed: Cell::new(false),
            reads: RefCell::new(std::collections::BTreeSet::new()),
            bytes: Cell::new(0),
        };
        let mut view = View::new(&effects);
        view.write_record(&mut record)?;
        if !effects.committed.get() {
            return Err(super::super::REFUSED.into());
        }
        effects.check()?;
        self.current_record(Some(data))?;
        self.store.check_writer(&self.state_lock, gate.writer()?)?;
        super::super::current(until)
    }
}

struct Publication<'a> {
    source: &'a Source,
    gate: &'a RunWriterGate,
    data: &'a Data,
    until: Instant,
    committed: Cell<bool>,
    failed: Cell<bool>,
    reads: RefCell<std::collections::BTreeSet<PathBuf>>,
    bytes: Cell<usize>,
}
impl Publication<'_> {
    fn checked<T>(&self, result: Result<T>) -> Result<T> {
        if result.is_err() {
            self.failed.set(true);
        }
        result
    }
}
impl Effects for Publication<'_> {
    fn check(&self) -> Result<()> {
        if self.failed.get() {
            return Err(super::super::REFUSED.into());
        }
        self.source.check()?;
        super::super::current(self.until)?;
        self.source
            .store
            .check_writer(&self.source.state_lock, self.gate.writer()?)?;
        self.source
            .current_record(self.committed.get().then_some(self.data))?;
        super::super::current(self.until)
    }
    fn until(&self) -> Instant {
        self.until
    }
    fn read(&self, path: &Path, limit: u64) -> Result<Option<ReadSource>> {
        self.check()?;
        let mut reads = self.reads.borrow_mut();
        if !reads.contains(path) && reads.len() >= 8 {
            return Err(super::super::REFUSED.into());
        }
        reads.insert(path.to_path_buf());
        drop(reads);
        let value = self.source.read(path, limit)?;
        let bytes = value.as_ref().map_or(0, |v| v.bytes().len());
        let total = self
            .bytes
            .get()
            .checked_add(bytes)
            .ok_or(super::super::REFUSED)?;
        if total > 4 * RECORD_BYTES {
            return Err(super::super::REFUSED.into());
        }
        self.bytes.set(total);
        Ok(value)
    }
    fn session_name(&self, id: &str) -> Result<Option<String>> {
        self.check()?;
        if self.source.binding["session_id"] != id {
            return Err(super::super::REFUSED.into());
        }
        Ok(Some(codec::text(&self.source.binding, "name", 512)?.into()))
    }
    fn atomic(&self, path: &Path, bytes: &[u8], sources: &[&ReadSource]) -> Result<()> {
        self.checked((|| {
            self.check()?;
            let path = self.source.selected(path)?;
            if bytes.len() > RECORD_BYTES {
                return Err(super::super::REFUSED.into());
            }
            let parent = Directory::open(path.parent().ok_or(super::super::REFUSED)?)?;
            if !parent.private()? {
                return Err(super::super::REFUSED.into());
            }
            let name = path
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or(super::super::REFUSED)?;
            publication::replace(&parent, name, bytes, || {
                for source in sources {
                    source.verify()?;
                }
                self.check()?;
                super::super::current(self.until)
            })?;
            if path == self.source.path {
                self.committed.set(true);
            }
            self.check()
        })())
    }
    fn remove(&self, _: &Path, _: &[&ReadSource]) -> Result<()> {
        Err(super::super::REFUSED.into())
    }
    fn restore(
        &self,
        _: &Path,
        _: &Path,
        _: &str,
        _: Option<&Value>,
        _: &[&ReadSource],
    ) -> Result<()> {
        Err(super::super::REFUSED.into())
    }
}
