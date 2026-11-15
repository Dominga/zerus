//! Opt-in upload frames inside the existing opaque update bytes. This is
//! document replication only: no native input or delivery result lives here.
use super::{sha256, MAX_ASSEMBLY, PAGE};
use crate::{ids, Document, Error, ProjectStore, Result, Update};
use same_file::Handle;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::Mutex,
    time::Duration,
};

const MAGIC: &[u8; 8] = b"ZERUSUP1";
const HEADER: usize = 1024;
const SLOT: &str = ".phone-upload";
/// An abandoned document assembly holds no input authority and can be
/// replaced after ten idle minutes. Active, different sources stay busy.
const IDLE: Duration = Duration::from_secs(600);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub project: String,
    pub task: String,
    pub sha256: String,
    pub size: usize,
}
impl Source {
    fn check(&self) -> Result<()> {
        if self.project.is_empty()
            || self.project.len() > 128
            || self.project.chars().any(char::is_control)
            || !ids::is_uuid(&self.task)
            || self.size == 0
            || self.size > MAX_ASSEMBLY
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(Error::new("invalid task upload source or size"));
        }
        Ok(())
    }
}

/// The exact frame received, including its requested end offset. A complete
/// acknowledgement means replica import was attempted; `refused` still applies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    pub source: Source,
    pub offset: usize,
    pub complete: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    source: Source,
    offset: usize,
}

pub(super) fn is_frame(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ZERUSUP")
}

fn encode(source: &Source, offset: usize, chunk: &[u8]) -> Result<Vec<u8>> {
    let header = serde_json::to_vec(&Header {
        source: source.clone(),
        offset,
    })
    .map_err(|e| Error::new(e.to_string()))?;
    if header.len() > HEADER {
        return Err(Error::new("task upload header is too large"));
    }
    let mut bytes = Vec::with_capacity(12 + header.len() + chunk.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(header.len() as u32).to_be_bytes());
    bytes.extend(header);
    bytes.extend_from_slice(chunk);
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<(Header, &[u8])> {
    let invalid = || Error::new("invalid task upload frame");
    if bytes.len() < 12 || &bytes[..8] != MAGIC {
        return Err(invalid());
    }
    let size = u32::from_be_bytes(bytes[8..12].try_into().map_err(|_| invalid())?) as usize;
    if size > HEADER || size > bytes.len() - 12 {
        return Err(invalid());
    }
    let header: Header = serde_json::from_slice(&bytes[12..12 + size]).map_err(|_| invalid())?;
    header.source.check()?;
    let chunk = &bytes[12 + size..];
    if chunk.is_empty()
        || header.offset > header.source.size
        || chunk.len() > header.source.size - header.offset
    {
        return Err(invalid());
    }
    Ok((header, chunk))
}

/// The frozen update stays byte-identical while the project receives edits.
pub(super) struct Outbound {
    pub source: Source,
    bytes: Vec<u8>,
    pub offset: usize,
}
impl Outbound {
    pub fn new(project: &str, task: &str, bytes: Vec<u8>) -> Result<Self> {
        let source = Source {
            project: project.into(),
            task: task.into(),
            size: bytes.len(),
            sha256: sha256(&bytes),
        };
        source.check()?;
        Ok(Self {
            source,
            bytes,
            offset: 0,
        })
    }
    pub fn frame(&self, budget: usize) -> Result<(Update, Ack)> {
        let budget = budget.min(PAGE);
        let overhead = encode(&self.source, self.offset, &[])?.len();
        let capacity = budget
            .checked_sub(overhead)
            .filter(|n| *n > 0)
            .ok_or_else(|| Error::new("the task upload budget is too small for framing"))?;
        let end = self.bytes.len().min(self.offset + capacity);
        let bytes = encode(&self.source, self.offset, &self.bytes[self.offset..end])?;
        Ok((
            Update {
                document: Document::Task(self.source.task.clone()),
                bytes,
            },
            Ack {
                source: self.source.clone(),
                offset: end,
                complete: end == self.bytes.len(),
            },
        ))
    }
}

/// Pin the opened project namespace, including across calls/threads. The
/// existing project lock excludes other processes; this serializes its slot.
pub(crate) struct Directory {
    pin: Handle,
    turn: Mutex<()>,
}
impl Directory {
    pub(crate) fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            pin: open(path, true)?,
            turn: Mutex::new(()),
        })
    }
    fn check(&self, path: &Path) -> Result<()> {
        if self.pin != open(path, true)? {
            return Err(Error::failed("the task upload directory changed"));
        }
        Ok(())
    }
}

fn open(path: &Path, directory: bool) -> Result<Handle> {
    let before = fs::symlink_metadata(path)?;
    if (directory && !before.is_dir()) || (!directory && !before.is_file()) {
        return Err(Error::failed(
            "the task upload path is not a plain file or directory",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            libc::O_NOFOLLOW | libc::O_NONBLOCK | if directory { libc::O_DIRECTORY } else { 0 },
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000 | if directory { 0x0200_0000 } else { 0 });
    }
    #[cfg(test)]
    testing::hook("before_open", path);
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if (directory && !meta.is_dir()) || (!directory && !meta.is_file()) {
        return Err(Error::failed("the task upload path changed type"));
    }
    #[cfg(unix)]
    if !directory {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 || meta.mode() & 0o077 != 0 {
            return Err(Error::failed("the task upload slot is not private"));
        }
    }
    Ok(Handle::from_file(file)?)
}

struct Slot {
    pin: Handle,
    source: Source,
    bytes: Vec<u8>,
    expired: bool,
}
fn read_slot(path: &Path) -> Result<Option<Slot>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let mut pin = open(path, false)?;
    let meta = pin.as_file().metadata()?;
    if meta.len() > (MAX_ASSEMBLY + HEADER + 12) as u64 {
        return Err(Error::failed("the task upload slot is too large"));
    }
    #[cfg(test)]
    testing::hook("before_read", path);
    let mut bytes = Vec::new();
    pin.as_file_mut()
        .take((MAX_ASSEMBLY + HEADER + 13) as u64)
        .read_to_end(&mut bytes)?;
    #[cfg(test)]
    testing::hook("after_read", path);
    if bytes.len() > MAX_ASSEMBLY + HEADER + 12 || pin != open(path, false)? {
        return Err(Error::failed("the task upload slot changed while reading"));
    }
    let (header, chunk) = decode(&bytes)?;
    if header.offset != 0 {
        return Err(Error::failed("invalid task upload slot offset"));
    }
    Ok(Some(Slot {
        pin,
        source: header.source,
        bytes: chunk.to_vec(),
        expired: meta
            .modified()?
            .elapsed()
            .is_ok_and(|elapsed| elapsed >= IDLE),
    }))
}
fn unchanged(path: &Path, slot: Option<&Slot>) -> Result<()> {
    match slot {
        Some(slot) if slot.pin == open(path, false)? => Ok(()),
        None if fs::symlink_metadata(path)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(())
        }
        _ => Err(Error::failed("the task upload slot changed")),
    }
}

fn save(
    store: &ProjectStore,
    path: &Path,
    slot: Option<&Slot>,
    source: &Source,
    bytes: &[u8],
) -> Result<()> {
    store.upload.check(store.dir())?;
    unchanged(path, slot)?;
    let temp = path.with_extension(format!("tmp-{}", ids::new_uuid()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    let temporary_pin = Handle::from_file(file.try_clone()?)?;
    let result = (|| -> Result<()> {
        file.write_all(&encode(source, 0, bytes)?)?;
        file.sync_all()?;
        store.upload.check(store.dir())?;
        unchanged(path, slot)?;
        let pin = &temporary_pin;
        #[cfg(test)]
        testing::hook("before_commit", &temp);
        if *pin != open(&temp, false)? {
            return Err(Error::failed("the task upload temporary file changed"));
        }
        fs::rename(&temp, path)?;
        #[cfg(test)]
        testing::hook("after_commit", path);
        if *pin != open(path, false)? {
            return Err(Error::failed("the task upload slot changed on commit"));
        }
        store.upload.check(store.dir())?;
        #[cfg(unix)]
        store.upload.pin.as_file().sync_all()?;
        Ok(())
    })();
    // Only this create_new temporary name is ours, never an unrelated slot.
    if result.is_err()
        && store.upload.check(store.dir()).is_ok()
        && open(&temp, false).is_ok_and(|current| current == temporary_pin)
    {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub(super) fn receive(store: &ProjectStore, update: Update) -> Result<(Ack, Option<Update>)> {
    let _turn = store
        .upload
        .turn
        .lock()
        .map_err(|_| Error::failed("the task upload slot lock failed"))?;
    store.upload.check(store.dir())?;
    if update.bytes.len() > PAGE {
        return Err(Error::new("the task upload frame exceeds its page budget"));
    }
    let (header, chunk) = decode(&update.bytes)?;
    let source = header.source;
    if source.project != store.project_id()
        || update.document != Document::Task(source.task.clone())
    {
        return Err(Error::new(
            "the task upload source does not match its project and task",
        ));
    }
    let end = header.offset + chunk.len();
    let path = store.dir().join(SLOT);
    let slot = read_slot(&path)?;
    let mut bytes = match &slot {
        Some(slot) if slot.source == source => slot.bytes.clone(),
        Some(slot) if slot.bytes.len() != slot.source.size && !slot.expired => {
            return Err(Error::failed(
                "another task upload is busy; retry document sync after it settles or ten idle minutes",
            ));
        }
        _ => Vec::new(),
    };
    if header.offset == bytes.len() {
        bytes.extend_from_slice(chunk);
    } else if end > bytes.len() || bytes[header.offset..end] != *chunk {
        return Err(Error::new(
            "the task upload offset or duplicate bytes do not match",
        ));
    }
    let complete = end == source.size;
    if complete && sha256(&bytes) != source.sha256 {
        return Err(Error::new("the complete task upload hash does not match"));
    }
    save(store, &path, slot.as_ref(), &source, &bytes)?;
    let update = complete.then(|| Update {
        document: Document::Task(source.task.clone()),
        bytes,
    });
    Ok((
        Ack {
            source,
            offset: end,
            complete,
        },
        update,
    ))
}

#[cfg(test)]
mod testing {
    use std::{cell::RefCell, path::Path};
    type Hook = (&'static str, Box<dyn FnOnce(&Path)>);
    thread_local! { static NEXT: RefCell<Option<Hook>> = const { RefCell::new(None) }; }
    pub fn on(phase: &'static str, action: impl FnOnce(&Path) + 'static) {
        NEXT.with(|next| *next.borrow_mut() = Some((phase, Box::new(action))));
    }
    pub fn hook(phase: &str, path: &Path) {
        let action = NEXT.with(|next| {
            if next
                .borrow()
                .as_ref()
                .is_some_and(|(wanted, _)| *wanted == phase)
            {
                next.borrow_mut().take()
            } else {
                None
            }
        });
        if let Some((_, action)) = action {
            action(path);
        }
    }
    pub fn clear() {
        NEXT.with(|next| {
            next.borrow_mut().take();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Source {
        Source {
            project: "project-1".into(),
            task: "11111111-1111-4111-8111-111111111111".into(),
            size: 1000,
            sha256: sha256(&[42; 1000]),
        }
    }
    fn frame() -> Update {
        Update {
            document: Document::Task(source().task),
            bytes: encode(&source(), 0, &[42; 40]).unwrap(),
        }
    }
    fn replace(path: &Path) {
        let temp = path.with_extension("replacement");
        fs::copy(path, &temp).unwrap();
        fs::rename(temp, path).unwrap();
    }
    #[test]
    fn source_identity_hash_shape_and_size_are_fixed_protocol_bounds() {
        assert!(source().check().is_ok());
        let mut cases = Vec::new();
        for project in ["".to_owned(), "x".repeat(129), "line\nfeed".into()] {
            let mut s = source();
            s.project = project;
            cases.push(s);
        }
        let mut s = source();
        s.task = "T-short1".into();
        cases.push(s);
        for size in [0, crate::MAX_TASK_DOCUMENT + 1] {
            let mut s = source();
            s.size = size;
            cases.push(s);
        }
        for hash in ["a".repeat(63), "G".repeat(64), "A".repeat(64)] {
            let mut s = source();
            s.sha256 = hash;
            cases.push(s);
        }
        for source in cases {
            assert!(source.check().is_err(), "{source:?}");
        }
        let mut edge = source();
        edge.size = crate::MAX_TASK_DOCUMENT;
        assert!(edge.check().is_ok());
    }
    #[test]
    fn truncated_unknown_empty_and_out_of_bounds_frames_are_refused() {
        let valid = encode(&source(), 0, &[42; 40]).unwrap();
        assert!(decode(&valid).is_ok());
        for n in [0, 7, 11, 12] {
            assert!(decode(&valid[..n]).is_err());
        }
        let mut future = valid.clone();
        future[7] = b'2';
        assert!(decode(&future).is_err());
        assert!(decode(&encode(&source(), 0, &[]).unwrap()).is_err());
        assert!(decode(&encode(&source(), usize::MAX, &[42]).unwrap()).is_err());
        assert!(decode(&encode(&source(), 999, &[42, 42]).unwrap()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn a_hard_link_to_a_valid_saved_slot_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        receive(&store, frame()).unwrap();
        let slot = store.dir().join(SLOT);
        let other = root.path().join("other-valid-copy");
        fs::hard_link(&slot, &other).unwrap();
        assert!(receive(&store, frame()).is_err());
        assert_eq!(fs::read(&slot).unwrap(), fs::read(&other).unwrap());
    }
    #[cfg(unix)]
    #[test]
    fn an_existing_slot_must_stay_private() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        receive(&store, frame()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(store.dir().join(SLOT), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(receive(&store, frame()).is_err());
    }
    #[test]
    fn unknown_source_header_and_ack_fields_are_not_extensions() {
        let mut source_json = serde_json::to_value(source()).unwrap();
        source_json["other"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Source>(source_json).is_err());
        let mut header = serde_json::to_value(Header {
            source: source(),
            offset: 0,
        })
        .unwrap();
        header["other"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Header>(header).is_err());
        let mut ack = serde_json::to_value(Ack {
            source: source(),
            offset: 40,
            complete: false,
        })
        .unwrap();
        ack["other"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Ack>(ack).is_err());
    }
    #[test]
    fn offsets_and_duplicate_bytes_are_checked_against_the_saved_prefix() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        let (ack, _) = receive(&store, frame()).unwrap();
        assert_eq!(receive(&store, frame()).unwrap().0, ack);
        for (offset, chunk) in [(41, vec![42; 40]), (39, vec![42; 40]), (0, vec![43; 40])] {
            let update = Update {
                document: Document::Task(source().task),
                bytes: encode(&source(), offset, &chunk).unwrap(),
            };
            assert!(receive(&store, update).is_err());
        }
    }
    #[test]
    fn header_length_is_bounded_before_deserializing() {
        let header = serde_json::to_vec(&Header {
            source: source(),
            offset: 0,
        })
        .unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&((HEADER + 1) as u32).to_be_bytes());
        bytes.extend(&header);
        bytes.resize(12 + HEADER + 1, b' ');
        bytes.push(42);
        assert!(decode(&bytes).is_err());
        bytes[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(decode(&bytes).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn a_directory_replaced_by_a_regular_file_is_refused_by_the_kernel() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("directory");
        fs::create_dir(&path).unwrap();
        testing::on("before_open", |path| {
            fs::rename(path, path.with_extension("held")).unwrap();
            fs::write(path, b"unrelated regular file").unwrap();
        });
        let error = open(&path, true)
            .expect_err("the replacement must not open")
            .to_string();
        assert!(
            error.contains(&format!("os error {}", libc::ENOTDIR)),
            "the regular file must never produce a descriptor: {error}"
        );
        testing::clear();
    }
    #[cfg(unix)]
    #[test]
    fn no_follow_applies_to_a_link_replaced_after_the_metadata_check() {
        use std::os::unix::fs::{symlink, OpenOptionsExt};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("slot");
        let external = root.path().join("other");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&external)
            .unwrap();
        testing::on("before_open", move |path| {
            fs::remove_file(path).unwrap();
            symlink(external, path).unwrap();
        });
        assert!(open(&path, false).is_err());
        testing::clear();
    }
    #[test]
    fn slot_identity_is_rechecked_after_a_read() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        receive(&store, frame()).unwrap();
        testing::on("after_read", replace);
        assert!(receive(&store, frame())
            .unwrap_err()
            .to_string()
            .contains("changed while reading"));
        testing::clear();
    }
    #[test]
    fn temporary_identity_is_checked_before_commit() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        testing::on("before_commit", replace);
        assert!(receive(&store, frame())
            .unwrap_err()
            .to_string()
            .contains("temporary file changed"));
        testing::clear();
        assert!(!store.dir().join(SLOT).exists());
    }
    #[test]
    fn installed_identity_is_checked_after_commit() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        testing::on("after_commit", replace);
        assert!(receive(&store, frame())
            .unwrap_err()
            .to_string()
            .contains("changed on commit"));
        testing::clear();
    }
    #[test]
    fn oversized_private_slot_is_refused_before_its_body_is_read() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        receive(&store, frame()).unwrap();
        OpenOptions::new()
            .write(true)
            .open(store.dir().join(SLOT))
            .unwrap()
            .set_len((MAX_ASSEMBLY + HEADER + 13) as u64)
            .unwrap();
        testing::on("before_read", |_| {
            panic!("oversized slot body must not be read")
        });
        assert!(receive(&store, frame()).is_err());
        testing::clear();
    }
}
