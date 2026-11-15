//! Only the fixed original spawn program constructs this live creation source.
//! No status, saved binding or reconnect can supply the retained writer/source.
use super::*;
use crate::state::{guarded_recovery::Original as Record, repositories::directory::Directory};
use std::{cell::Cell, path::Path, time::Instant};

const REFUSED: &str = "Original writer-domain creation is unconfirmed; review without replay";
const RECORD_BYTES: u64 = 1 << 20;

fn deadline(until: Instant) -> Result<()> {
    if Instant::now() >= until {
        Err(REFUSED.into())
    } else {
        Ok(())
    }
}
fn routes(source: &native::Source, state: &Path, config: &Path) -> Result<()> {
    let (native_state, native_config, _) = source.directories();
    if !native::profile()
        || !source.current()
        || !state.is_absolute()
        || !config.is_absolute()
        || crate::platform::canonical(state).map_err(|_| REFUSED)? != native_state
        || crate::platform::canonical(config).map_err(|_| REFUSED)? != native_config
    {
        return Err(REFUSED.into());
    }
    Ok(())
}
fn transitions(record: &Value) -> Result<()> {
    if [
        "terminating",
        "pausing",
        "layout_transition",
        "rename_shadow",
        "rename_target",
    ]
    .iter()
    .any(|key| record.get(*key).is_some())
    {
        return Err(REFUSED.into());
    }
    Ok(())
}
fn read_file(path: &Path, limit: u64) -> Result<Option<Record>> {
    // Resolve only the configured root alias. Internal metadata parents must
    // remain real pinned children, never another independently resolved alias.
    let configured = storage::root();
    let relative = path.strip_prefix(&configured).map_err(|_| REFUSED)?;
    let parts: Vec<_> = relative.components().collect();
    if parts.is_empty()
        || parts.len() > 2
        || parts
            .iter()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
    {
        return Err(REFUSED.into());
    }
    let root = Directory::open(&crate::platform::canonical(&configured).map_err(|_| REFUSED)?)?;
    let parent = if parts.len() == 2 {
        let part = parts[0].as_os_str().to_str().ok_or(REFUSED)?;
        match std::fs::symlink_metadata(root.path.join(part)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(REFUSED.into()),
            Ok(_) => root.child(part)?,
        }
    } else {
        Directory::open(&root.path)?
    };
    let selected = parent.path.join(parts.last().ok_or(REFUSED)?.as_os_str());
    let Some(original) = Record::read_existing(&selected, limit)? else {
        parent.verify()?;
        root.verify()?;
        return Ok(None);
    };
    if !original
        .file()
        .is_some_and(|file| crate::platform::verify_private_file(file).unwrap_or(false))
    {
        return Err(REFUSED.into());
    }
    original.verify()?;
    parent.verify()?;
    root.verify()?;
    Ok(Some(original))
}
fn read(path: &Path, name: &str) -> Result<Option<(Record, Value)>> {
    let Some(original) = read_file(path, RECORD_BYTES)? else {
        return Ok(None);
    };
    let record = terminating::creation_record(original.bytes())?;
    storage::verify_metadata_at(path, name, original.bytes())?;
    transitions(&record)?;
    original.verify()?;
    Ok(Some((original, record)))
}

pub(super) fn reserve(
    source: &native::Source,
    writer: &storage::Guard,
    record: &mut Value,
    binding: &Value,
    until: Instant,
) -> Result<()> {
    deadline(until)?;
    let state = storage::root();
    let config = crate::config::config_dir(&home().to_string_lossy());
    routes(source, &state, &config)?;
    let (proof, current) =
        read(&storage::own_path(record)?, string(record, "name"))?.ok_or(REFUSED)?;
    if current != *record || current.get("original_owner").is_some() {
        return Err(REFUSED.into());
    }
    let root = Directory::open(source.directories().0)?;
    let lock = root.existing_private_lock(".lock")?;
    if crate::platform::FileIdentity::of(&lock).map_err(|_| REFUSED)?
        != writer.identity().map_err(|_| REFUSED)?
    {
        return Err(REFUSED.into());
    }
    proof.verify()?;
    routes(source, &state, &config)?;
    deadline(until)?;
    record["original_owner"] = binding.clone();
    storage::write(record)
}

/// Existing-only, bounded admission without a fleet scan or unrelated recovery.
/// The fixed supervisor receives the canonical SID from its launch program.
pub(super) fn begin(
    name: &str,
    run: &str,
    until: Instant,
) -> Result<(storage::Guard, native::Source, Value)> {
    deadline(until)?;
    let state = storage::root();
    let config = crate::config::config_dir(&home().to_string_lossy());
    let source = native::Source::capture(&state, &config).map_err(|e| e.to_string())?;
    routes(&source, &state, &config)?;
    let writer = storage::existing_lock_until(&source.directories().0.join(".lock"), until)?
        .ok_or(REFUSED)?;
    let sid = std::env::var("HGS_SESSION_ID").map_err(|_| REFUSED)?;
    let id = storage::id_record_path(&sid).ok_or(REFUSED)?;
    let (proof, record) = if let Some(found) = read(&id, name)? {
        found
    } else {
        // Read this one route only. Missing names never trigger an ID scan.
        let route = read_file(&storage::route_path(name), 4096)?;
        let legacy = storage::legacy_record_path(name);
        let path = if route.is_some() {
            state
                .join("bindings")
                .join(legacy.file_name().ok_or(REFUSED)?)
        } else {
            legacy
        };
        let found = read(&path, name)?.ok_or(REFUSED)?;
        if let Some(route) = route {
            route.verify()?;
        }
        found
    };
    if record["session_id"] != sid
        || record["run_id"] != run
        || record.get("original_owner").is_some()
    {
        return Err(REFUSED.into());
    }
    rename::claim_ready(&state, &record, name)?;
    proof.verify()?;
    routes(&source, &state, &config)?;
    deadline(until)?;
    Ok((writer, source, record))
}

#[cfg(test)]
mod tests;

/// Non-cloneable and non-deserializable. Its constructor is private to the
/// original spawn program, after durable publication of creation uncertainty.
pub(in crate::state) struct CreationSource<'a> {
    source: &'a native::Source,
    writer: &'a storage::Guard,
    original: Record,
    record: Value,
    state: PathBuf,
    config: PathBuf,
    until: Instant,
    attempted: Cell<bool>,
}
impl<'a> CreationSource<'a> {
    pub(super) fn capture(
        source: &'a native::Source,
        writer: &'a storage::Guard,
        record: &Value,
        until: Instant,
    ) -> Result<Self> {
        let state = storage::root();
        let config = crate::config::config_dir(&home().to_string_lossy());
        let path = storage::own_path(record)?;
        let (original, current) = read(&path, string(record, "name"))?.ok_or(REFUSED)?;
        if current != *record {
            return Err(REFUSED.into());
        }
        let binding = &record["original_owner"];
        if binding["owner"] != std::process::id()
            || binding["owner_birth"] != source.birth()
            || binding["source"] != source.wire()
            || binding["session"] != record["session_id"]
            || binding["run"] != record["run_id"]
            || !binding["nonce"].as_str().is_some_and(|s| {
                uuid::Uuid::parse_str(s)
                    .is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == s)
            })
            || binding.get("root").is_some()
        {
            return Err(REFUSED.into());
        }
        let value = Self {
            source,
            writer,
            original,
            record: current,
            state,
            config,
            until,
            attempted: Cell::new(false),
        };
        value.check()?;
        Ok(value)
    }
    pub(in crate::state) fn check(&self) -> Result<()> {
        deadline(self.until)?;
        if storage::root() != self.state
            || crate::config::config_dir(&home().to_string_lossy()) != self.config
        {
            return Err(REFUSED.into());
        }
        routes(self.source, &self.state, &self.config)?;
        let root = self.root()?;
        let file = root.existing_private_lock(".lock")?;
        if crate::platform::FileIdentity::of(&file).map_err(|_| REFUSED)?
            != self.writer.identity().map_err(|_| REFUSED)?
        {
            return Err(REFUSED.into());
        }
        self.original.verify()?;
        transitions(&self.record)?;
        rename::claim_ready(&self.state, &self.record, string(&self.record, "name"))?;
        deadline(self.until)
    }
    pub(in crate::state) fn start(&self) -> Result<()> {
        if self.attempted.replace(true) {
            return Err(REFUSED.into());
        }
        self.check()
    }
    pub(in crate::state) fn root(&self) -> Result<Directory> {
        let (path, _, expected) = self.source.directories();
        let root = Directory::open(path)?;
        use std::os::fd::FromRawFd;
        // Retain/compare the descriptor from the original native root, never a
        // renderer/stored identity. dup is read-only; no extra lock is acquired.
        let fd = unsafe { libc::dup(root.descriptor()) };
        if fd < 0 {
            return Err(REFUSED.into());
        }
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let actual = crate::platform::FileIdentity::of(&file).map_err(|_| REFUSED)?;
        if (actual.device, actual.inode) != (expected.device, expected.inode) || !root.private()? {
            return Err(REFUSED.into());
        }
        Ok(root)
    }
    pub(in crate::state) fn scope(&self) -> (&str, &str) {
        (
            string(&self.record, "session_id"),
            string(&self.record, "run_id"),
        )
    }
    pub(in crate::state) fn configured_root(&self) -> &Path {
        &self.state
    }
    pub(in crate::state) fn until(&self) -> Instant {
        self.until
    }
}
