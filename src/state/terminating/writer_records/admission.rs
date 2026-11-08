//! Inactive original-request/run admission. This is correlation/exclusion DATA,
//! not native image/configuration/target/consent proof or an operation receipt.
//! Future hosts must validate their ORIGINAL durable request (including no
//! replay) and native source before using this single-use local object. No
//! producer, migration, mutator, helper launch or graceful controller calls it.
use super::store::Directory;
use super::*;
use crate::{platform, state::storage};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const RECORD_BYTES: usize = 1 << 20;
const BINDING_BYTES: usize = 32 << 10;

/// The exact host-validated request correlation. Hex digests do NOT establish
/// source authority; the real host must retain its native image/configuration
/// checks. This type has no deserializer or execution/target-selection entry.
pub(super) struct Request {
    id: String,
    kind: Kind,
    source_sha256: String,
    intent_sha256: String,
}
impl Request {
    pub(super) fn new(id: &str, kind: Kind, source: &str, intent: &str) -> Result<Self> {
        if !uuid(id) || !digest(source) || !digest(intent) {
            return Err(REFUSED.into());
        }
        Ok(Self {
            id: id.into(),
            kind,
            source_sha256: source.into(),
            intent_sha256: intent.into(),
        })
    }
}

/// One captured attempt, consumed by reserve. Saved IDs/rows cannot reconstruct
/// this live source/owner. Re-creating data is never a durable no-replay receipt.
pub(super) struct OriginalRequest {
    source: Source,
    request: Request,
}
impl OriginalRequest {
    pub(super) fn capture(
        writer: &storage::Guard,
        record: &Value,
        request: Request,
        until: Instant,
    ) -> Result<Self> {
        let now = Instant::now();
        if until <= now || until.duration_since(now) > Duration::from_secs(30) {
            return Err(REFUSED.into());
        }
        let binding = Binding::from_record(record)?;
        let run = Run::new(
            text(&binding.0, "session_id", 36, false)?,
            text(&binding.0, "run_id", 36, false)?,
        )?;
        let store = Store::open(&run, until)?;
        let state_lock = store.state_lock()?;
        store.check_writer(&state_lock, writer)?;
        let source = Source {
            record: Record::open(record)?,
            binding,
            store,
            state_lock,
            owner: Owner::current()?,
            until,
        };
        source.recheck(writer)?;
        Ok(Self { source, request })
    }
    pub(super) fn reserve(self, writer: &storage::Guard) -> Result<Reserved> {
        self.source.recheck(writer)?;
        let binding_sha256 = hash(
            &serde_json::json!({"record":self.source.binding.0,"source_sha256":self.request.source_sha256}),
            BINDING_BYTES,
        )?;
        let entry = Entry {
            request: self.request.id,
            kind: self.request.kind,
            binding_sha256,
            intent_sha256: self.request.intent_sha256,
            owner: self.source.owner.clone(),
        };
        // This original shared gate is tried without waiting while the normal
        // writer is held. The store still refuses duplicates/damage/capacity.
        let reservation = self
            .source
            .store
            .clone()
            .reserve(entry, self.source.until)?;
        #[cfg(test)]
        AFTER_RESERVE.with(|slot| {
            if let Some(change) = slot.borrow_mut().take() {
                change();
            }
        });
        // A late refusal preserves its pending row; no implicit recovery/erase.
        self.source.recheck(writer)?;
        Ok(Reserved {
            source: self.source,
            reservation,
        })
    }
}

pub(super) struct Reserved {
    source: Source,
    reservation: Reservation,
}
impl Reserved {
    /// Keep the source for last-boundary checks while lending the ORIGINAL
    /// reservation to the existing I/O bridge. Neither part grants input.
    pub(super) fn into_parts(self) -> (Source, Reservation) {
        (self.source, self.reservation)
    }
}

/// Read-only original source companion. Rechecking requires a current normal
/// writer on the SAME pinned state-lock object; it never initializes storage,
/// repairs/reconciles the record, selects another binding or renews the budget.
pub(super) struct Source {
    store: Store,
    record: Record,
    binding: Binding,
    state_lock: File,
    owner: Owner,
    until: Instant,
}
impl Source {
    pub(super) fn recheck(&self, writer: &storage::Guard) -> Result<()> {
        current(self.until)?;
        if Owner::current()? != self.owner {
            return Err(REFUSED.into());
        }
        self.store.check_writer(&self.state_lock, writer)?;
        if Binding::from_record(&self.record.read(self.until)?)? != self.binding {
            return Err(REFUSED.into());
        }
        self.store.check_writer(&self.state_lock, writer)?;
        current(self.until)
    }
}

struct Record {
    parent: Directory,
    path: PathBuf,
}
impl Record {
    fn open(record: &Value) -> Result<Self> {
        let root = storage::root();
        let path = storage::own_path(record).map_err(|_| REFUSED)?;
        let relative = path.strip_prefix(&root).map_err(|_| REFUSED)?;
        let parent = relative.parent().ok_or(REFUSED)?;
        if !matches!(parent.to_str(), Some("" | "sessions" | "bindings")) {
            return Err(REFUSED.into());
        }
        let path = platform::canonical(&root)
            .map_err(|_| REFUSED)?
            .join(relative);
        let parent = Directory::open(path.parent().ok_or(REFUSED)?)?;
        Ok(Self { parent, path })
    }
    fn read(&self, until: Instant) -> Result<Value> {
        current(until)?;
        self.parent.current()?;
        let file = platform::no_follow(OpenOptions::new().read(true))
            .open(&self.path)
            .map_err(|_| REFUSED)?;
        if !platform::verify_private_file(&file).map_err(|_| REFUSED)?
            || file.metadata().map_err(|_| REFUSED)?.len() > RECORD_BYTES as u64
        {
            return Err(REFUSED.into());
        }
        let identity = platform::FileIdentity::of(&file).map_err(|_| REFUSED)?;
        let length = file.metadata().map_err(|_| REFUSED)?.len();
        let mut bytes = Vec::new();
        (&file)
            .take(RECORD_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| REFUSED)?;
        if bytes.len() > RECORD_BYTES {
            return Err(REFUSED.into());
        }
        #[cfg(test)]
        AFTER_RECORD_READ.with(|slot| {
            if let Some(change) = slot.borrow_mut().take() {
                change();
            }
        });
        // Atomic completion rewrites BETWEEN checks are allowed. During this
        // read the path must still name the original, unchanged open file.
        let current_file = platform::no_follow(OpenOptions::new().read(true))
            .open(&self.path)
            .map_err(|_| REFUSED)?;
        if !platform::verify_private_file(&current_file).map_err(|_| REFUSED)?
            || platform::FileIdentity::of(&file).map_err(|_| REFUSED)? != identity
            || platform::FileIdentity::of(&current_file).map_err(|_| REFUSED)? != identity
            || file.metadata().map_err(|_| REFUSED)?.len() != length
            || current_file.metadata().map_err(|_| REFUSED)?.len() != length
            || bytes.len() as u64 != length
        {
            return Err(REFUSED.into());
        }
        self.parent.current()?;
        current(until)?;
        serde_json::from_slice(&bytes).map_err(|_| REFUSED.into())
    }
}

/// Only original binding fields; saved PID/start remain correlation, NOT native
/// birth/ancestry proof. Completion/attention metadata may legitimately advance.
#[derive(PartialEq)]
struct Binding(Value);
impl Binding {
    fn from_record(record: &Value) -> Result<Self> {
        if !record.is_object() || record["version"] != 1 || super::super::pending(record) {
            return Err(REFUSED.into());
        }
        let mut fields = Map::new();
        for (key, limit) in [
            ("name", 512),
            ("session_id", 36),
            ("run_id", 36),
            ("agent", 64),
            ("pane", 256),
            ("terminal_name", 512),
            ("process_start", 128),
            ("cwd", 4096),
            ("launch_dir", 4096),
            ("hgs", 4096),
        ] {
            fields.insert(
                key.into(),
                Value::String(text(record, key, limit, false)?.into()),
            );
        }
        Run::new(
            text(record, "session_id", 36, false)?,
            text(record, "run_id", 36, false)?,
        )?;
        for key in ["cwd", "launch_dir", "hgs"] {
            if !Path::new(text(record, key, 4096, false)?).is_absolute() {
                return Err(REFUSED.into());
            }
        }
        let conversation = record.get("conversation_id").ok_or(REFUSED)?;
        if !conversation.is_null() {
            text(record, "conversation_id", 1024, true)?;
        }
        fields.insert("conversation_id".into(), conversation.clone());
        fields.insert("pid".into(), serde_json::json!(pid(&record["pid"])?));
        let supervisor = record.get("supervisor").ok_or(REFUSED)?;
        let start = text(supervisor, "start", 128, false)?;
        fields.insert(
            "supervisor".into(),
            serde_json::json!({"pid":pid(&supervisor["pid"])?,"start":start}),
        );
        if let Some(layout) = record.get("layout") {
            if layout != "id" {
                return Err(REFUSED.into());
            }
            fields.insert("layout".into(), layout.clone());
        }
        if let Some(owner) = record.get("worktree_owner") {
            bounded_owner(owner)?;
            fields.insert("worktree_owner".into(), owner.clone());
        }
        let result = Self(Value::Object(fields));
        let _ = encoded(&result.0, BINDING_BYTES)?;
        Ok(result)
    }
}
fn text<'a>(value: &'a Value, key: &str, max: usize, empty: bool) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| (empty || !v.is_empty()) && v.len() <= max && !v.chars().any(char::is_control))
        .ok_or_else(|| REFUSED.into())
}
fn pid(value: &Value) -> Result<u32> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v > 0)
        .ok_or_else(|| REFUSED.into())
}
fn bounded_owner(value: &Value) -> Result<()> {
    let mut stack = vec![(value, 0)];
    let mut count = 0;
    while let Some((value, depth)) = stack.pop() {
        count += 1;
        if count > 64 || depth > 8 {
            return Err(REFUSED.into());
        }
        match value {
            Value::Array(items) => {
                if count + stack.len() + items.len() > 64 {
                    return Err(REFUSED.into());
                }
                stack.extend(items.iter().map(|v| (v, depth + 1)));
            }
            Value::Object(items) => {
                if count + stack.len() + items.len() > 64 {
                    return Err(REFUSED.into());
                }
                stack.extend(items.values().map(|v| (v, depth + 1)));
            }
            _ => {}
        }
    }
    encoded(value, 8 << 10).map(drop)
}
fn hash(value: &Value, cap: usize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(encoded(value, cap)?)))
}
fn encoded(value: &Value, cap: usize) -> Result<Vec<u8>> {
    struct Capped {
        bytes: Vec<u8>,
        cap: usize,
    }
    impl Write for Capped {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.cap.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("bounded source"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Capped {
        bytes: Vec::new(),
        cap,
    };
    serde_json::to_writer(&mut out, value).map_err(|_| REFUSED)?;
    Ok(out.bytes)
}

#[cfg(test)]
thread_local! {
    static AFTER_RESERVE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static AFTER_RECORD_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests;
