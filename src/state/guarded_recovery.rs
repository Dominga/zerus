//! Data proofs and explicit effects for recovery under one supplied Root owner.
//! No object here acquires that Root, mints a native lease or executes a helper.
use super::repositories::directory::Directory;
use super::*;
use std::{fs::File, io::Read, os::unix::fs::MetadataExt, sync::Arc, time::Instant};

/// A restriction implemented by the original keeper. Implementations latch
/// refusal, stage under original parents, reverify every source after staging,
/// and check the original EOF/deadline LAST before the native commit. Errors
/// never authorize retry or claim that an admitted write did not commit.
pub(in crate::state) trait Effects {
    fn check(&self) -> Result<()>;
    fn until(&self) -> Instant;
    fn read(&self, path: &Path, limit: u64) -> Result<Option<Original>>;
    fn session_name(&self, id: &str) -> Result<Option<String>>;
    fn atomic(&self, path: &Path, bytes: &[u8], sources: &[&Original]) -> Result<()>;
    fn remove(&self, path: &Path, sources: &[&Original]) -> Result<()>;
    fn restore(
        &self,
        lock: &Path,
        taken: &Path,
        sid: &str,
        object: Option<&Value>,
        sources: &[&Original],
    ) -> Result<()>;
}

fn version(file: &File) -> Result<Value> {
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() {
        return Err("Recovery source is not a regular file".into());
    }
    Ok(json!([
        m.dev(),
        m.ino(),
        m.mode(),
        m.uid(),
        m.gid(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec()
    ]))
}

struct Held {
    path: PathBuf,
    anchor: Directory,
    /// Components below the SAME existing parent, including the filename.
    relative: PathBuf,
    file: Option<File>,
    version: Option<Value>,
    bytes: Vec<u8>,
}

/// Existing-only read or exact absence. Arc clones retain the same native file
/// and directory; they are data evidence, never an exclusion or effect permit.
#[derive(Clone)]
pub(in crate::state) struct Original(Arc<Held>);

impl Original {
    pub(in crate::state) fn path(&self) -> &Path {
        &self.0.path
    }
    pub(in crate::state) fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }
    pub(in crate::state) fn version(&self) -> Option<&Value> {
        self.0.version.as_ref()
    }
    pub(in crate::state) fn is_present(&self) -> bool {
        self.0.file.is_some()
    }
    /// SAME read-only data object, for an original-owner primitive's known
    /// own hard-link transition. It carries no writer/continuation authority.
    pub(in crate::state) fn file(&self) -> Option<&File> {
        self.0.file.as_ref()
    }

    pub(in crate::state) fn read_existing(path: &Path, limit: u64) -> Result<Option<Self>> {
        let held = Self::capture(path, limit)?;
        Ok(held.is_present().then_some(held))
    }

    pub(in crate::state) fn read_absent(path: &Path) -> Result<Self> {
        let held = Self::capture(path, 0)?;
        if held.is_present() {
            return Err("Recovery source appeared after its absent read".into());
        }
        Ok(held)
    }

    fn capture(path: &Path, limit: u64) -> Result<Self> {
        if !path.is_absolute() || limit > storage::ID_RECORD_BYTES {
            return Err("Invalid bounded recovery source".into());
        }
        let mut parent = path.parent().ok_or("Invalid recovery source parent")?;
        loop {
            match std::fs::symlink_metadata(parent) {
                Ok(meta) if meta.is_dir() || meta.file_type().is_symlink() => break,
                Ok(_) => return Err("Recovery source parent is not a directory".into()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    parent = parent.parent().ok_or("Missing recovery source root")?;
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        // Resolve existing aliases once, then retain/check their selected route
        // and walk all remaining components through pinned no-follow parents.
        let canonical = crate::platform::canonical(parent).map_err(|e| e.to_string())?;
        let anchor = Directory::open(&canonical)?;
        let relative = path
            .strip_prefix(parent)
            .map_err(|e| e.to_string())?
            .to_path_buf();
        let file = open_below(&anchor, &relative)?;
        let expected = file.as_ref().map(version).transpose()?;
        let mut bytes = Vec::new();
        if let Some(file) = &file {
            if file.metadata().map_err(|e| e.to_string())?.len() > limit {
                return Err("Recovery source exceeds its bound".into());
            }
            file.take(limit + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > limit {
                return Err("Recovery source exceeds its bound".into());
            }
        }
        let held = Self(Arc::new(Held {
            path: path.to_path_buf(),
            anchor,
            relative,
            file,
            version: expected,
            bytes,
        }));
        held.verify()?;
        Ok(held)
    }

    pub(in crate::state) fn verify(&self) -> Result<()> {
        self.0.anchor.verify()?;
        let parent = self
            .0
            .path
            .ancestors()
            .nth(self.0.relative.components().count())
            .ok_or("Invalid original recovery route")?;
        if crate::platform::canonical(parent).map_err(|e| e.to_string())? != self.0.anchor.path {
            return Err("Original recovery parent route changed".into());
        }
        if self.0.file.as_ref().map(version).transpose()? != self.0.version
            || open_below(&self.0.anchor, &self.0.relative)?
                .as_ref()
                .map(version)
                .transpose()?
                != self.0.version
        {
            return Err("Original recovery source or absence changed".into());
        }
        self.0.anchor.verify()
    }

    fn lock_file(&self) -> Result<File> {
        self.verify()?;
        self.0
            .file
            .as_ref()
            .ok_or("Recovery auxiliary exclusion is missing")?
            .try_clone()
            .map_err(|e| e.to_string())
    }
}

fn open_below(anchor: &Directory, relative: &Path) -> Result<Option<File>> {
    use std::os::fd::FromRawFd;
    let mut components = relative.components().peekable();
    let mut parent = None;
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            return Err("Invalid recovery component".into());
        };
        let directory = parent.as_ref().unwrap_or(anchor);
        if components.peek().is_some() {
            match std::fs::symlink_metadata(directory.path.join(name)) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.to_string()),
                Ok(_) => {
                    parent =
                        Some(directory.child(name.to_str().ok_or("Non-UTF8 recovery directory")?)?);
                }
            }
        } else {
            let name = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(name))
                .map_err(|e| e.to_string())?;
            // SAME pinned parent, final no-follow/nonblocking regular-file read.
            let raw = unsafe {
                libc::openat(
                    directory.descriptor(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if raw < 0 {
                let e = io::Error::last_os_error();
                return if e.kind() == io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(e.to_string())
                };
            }
            let file = unsafe { File::from_raw_fd(raw) };
            version(&file)?;
            directory.verify()?;
            return Ok(Some(file));
        }
    }
    Err("Recovery source has no filename".into())
}

/// Bounded data view shared through one recovery transaction. A successful
/// own write refreshes just that proof; a failed/uncertain write never does.
pub(in crate::state) struct View<'a> {
    effects: &'a dyn Effects,
    sources: BTreeMap<PathBuf, Original>,
    bytes: usize,
}
impl<'a> View<'a> {
    pub(in crate::state) fn new(effects: &'a dyn Effects) -> Self {
        Self {
            effects,
            sources: BTreeMap::new(),
            bytes: 0,
        }
    }
    pub(in crate::state) fn until(&self) -> Instant {
        self.effects.until()
    }
    pub(in crate::state) fn check(&self) -> Result<()> {
        self.effects.check()?;
        if Instant::now() >= self.effects.until() {
            return Err("Original recovery deadline passed".into());
        }
        Ok(())
    }
    pub(in crate::state) fn read(&mut self, path: &Path, limit: u64) -> Result<Option<Vec<u8>>> {
        self.check()?;
        if let Some(original) = self.sources.get(path) {
            original.verify()?;
            if original.bytes().len() as u64 > limit {
                return Err("Recovery source exceeds its requested bound".into());
            }
            self.check()?;
            return Ok(original.is_present().then(|| original.bytes().to_vec()));
        }
        let original = self
            .effects
            .read(path, limit)?
            .map(Ok)
            .unwrap_or_else(|| Original::read_absent(path))?;
        self.check()?;
        self.bytes = self
            .bytes
            .checked_add(original.bytes().len())
            .ok_or("Recovery source bytes overflow")?;
        if self.sources.len() >= 4096 || self.bytes > 64 << 20 {
            return Err("Recovery source inventory exceeds its bound".into());
        }
        let value = original.is_present().then(|| original.bytes().to_vec());
        self.sources.insert(path.to_path_buf(), original);
        Ok(value)
    }
    pub(in crate::state) fn json(&mut self, path: &Path, limit: u64) -> Result<Option<Value>> {
        self.read(path, limit)?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
            .transpose()
    }
    pub(in crate::state) fn label_path(&mut self, name: &str) -> Result<PathBuf> {
        let legacy = storage::legacy_record_path(name);
        Ok(if self.read(&storage::route_path(name), 4096)?.is_some() {
            root()
                .join("bindings")
                .join(legacy.file_name().ok_or("Invalid legacy record path")?)
        } else {
            legacy
        })
    }
    pub(in crate::state) fn record(&mut self, path: &Path, name: &str) -> Result<Value> {
        let bytes = self
            .read(path, storage::ID_RECORD_BYTES)?
            .ok_or("Recovery record is missing")?;
        let mut record = storage::verify_metadata_at(path, name, &bytes)?;
        // Only pure historical hook repairs apply here. Optional external
        // telemetry/lifecycle refresh can publish caches or open agent-home
        // SQLite/process sources. Retain stored fields without inventing
        // idle/empty/completion; the admitted Effort Scope still performs
        // all ordinary native readiness checks afterward.
        journal::repair_kimi_main(&mut record);
        journal::repair_claude_internal(&mut record);
        self.check()?;
        Ok(record)
    }
    pub(in crate::state) fn atomic_record(
        &mut self,
        path: &Path,
        record: &mut Value,
    ) -> Result<()> {
        storage::ensure_session_id(record);
        processes::compact(record);
        self.atomic(path, format!("{record}\n").as_bytes())
        // No native companion note or SQLite reconciliation here. JSON is
        // authoritative. Its changed native version leaves old companion/DB
        // coverage stale; removal refuses until ordinary reconciliation.
    }
    pub(in crate::state) fn write_record(&mut self, record: &mut Value) -> Result<()> {
        record["updated"] = json!(now());
        let at_id = storage::id_record_path(string(record, "session_id"));
        let existing = match &at_id {
            Some(p) => self.read(p, storage::ID_RECORD_BYTES)?.is_some(),
            None => false,
        };
        let by_id = storage::in_id_layout(record);
        let path = match (by_id, existing) {
            (true, true) => at_id.ok_or("Missing recovery session path")?,
            (false, false) => self.label_path(string(record, "name"))?,
            _ => return Err("Recovery session layout and record disagree".into()),
        };
        self.atomic_record(&path, record)?;
        session_index::note_guarded(
            self,
            record,
            if by_id {
                session_index::Kind::IdRecord
            } else {
                session_index::Kind::Record
            },
        )?;
        if !by_id {
            rename::mirror_legacy_guarded(self, record)?;
        }
        Ok(())
    }
    pub(in crate::state) fn files(&self, folder: &Path, cap: usize) -> Result<Vec<PathBuf>> {
        Ok(self
            .entries(folder, cap)?
            .into_iter()
            .filter(|path| path.extension().is_some_and(|v| v == "json"))
            .collect())
    }
    pub(in crate::state) fn entries(&self, folder: &Path, cap: usize) -> Result<Vec<PathBuf>> {
        self.check()?;
        let entries = match std::fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        let mut paths = Vec::new();
        for (count, entry) in entries.enumerate() {
            self.check()?;
            if count >= cap {
                return Err("Recovery enumeration exceeds its bound".into());
            }
            paths.push(entry.map_err(|e| e.to_string())?.path());
        }
        paths.sort();
        self.check()?;
        Ok(paths)
    }
    fn refresh(&mut self, path: &Path) -> Result<()> {
        if let Some(old) = self.sources.remove(path) {
            self.bytes -= old.bytes().len();
        }
        self.read(path, storage::ID_RECORD_BYTES)?;
        Ok(())
    }
    pub(in crate::state) fn atomic(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.read(path, storage::ID_RECORD_BYTES)?;
        self.check()?;
        self.effects
            .atomic(path, bytes, &self.sources.values().collect::<Vec<_>>())?;
        self.refresh(path)
    }
    pub(in crate::state) fn remove(&mut self, path: &Path) -> Result<()> {
        self.read(path, storage::ID_RECORD_BYTES)?;
        self.check()?;
        self.effects
            .remove(path, &self.sources.values().collect::<Vec<_>>())?;
        self.refresh(path)
    }
    pub(in crate::state) fn restore(
        &mut self,
        lock: &Path,
        taken: &Path,
        sid: &str,
        object: Option<&Value>,
    ) -> Result<()> {
        self.read(lock, 4096)?;
        self.read(taken, 4096)?;
        self.check()?;
        self.effects.restore(
            lock,
            taken,
            sid,
            object,
            &self.sources.values().collect::<Vec<_>>(),
        )?;
        self.refresh(lock)?;
        self.refresh(taken)
    }
    pub(in crate::state) fn session_name(&self, id: &str) -> Result<Option<String>> {
        self.check()?;
        let value = self.effects.session_name(id)?;
        for source in self.sources.values() {
            source.verify()?;
        }
        self.check()?;
        Ok(value)
    }
    pub(in crate::state) fn auxiliary(&mut self, path: &Path) -> Result<storage::Guard> {
        use fs2::FileExt;
        if self.read(path, 4096)?.is_none() {
            self.atomic(path, b"")?;
        }
        let original = self
            .sources
            .get(path)
            .ok_or("Original recovery exclusion is unavailable")?;
        let file = original.lock_file()?;
        loop {
            self.check()?;
            original.verify()?;
            match file.try_lock_exclusive() {
                Ok(()) => {
                    let guard = storage::Guard::held(file);
                    self.check()?;
                    original.verify()?;
                    return Ok(guard);
                }
                Err(e) if crate::platform::lock_contended(&e) => {
                    std::thread::sleep(std::time::Duration::from_millis(2))
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests;
