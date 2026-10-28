//! Downloaded copies of session files (tray/src/DownloadCache.{h,cpp}): the
//! folder Download and open saves into, a verified copy found again by its
//! source, and the least recently used copies removed past a size limit.
//!
//! The folder (by default `<Downloads>/Zerus Files`) is shared with the Qt
//! client by design, so its formats are Qt's byte for byte: the index
//! `.zerus-files.json` (version 1), in-progress `.zerus-part-*` files, and
//! the `.zerus-files.lock` QLockFile ([`crate::qt_lock`]) that serializes
//! every change across both apps. The index is written atomically (a temporary
//! file, then a rename); an index that cannot be read, is too large, a link
//! or of another version is never rewritten, trimmed or removed from.
//!
//! Every function blocks on the file system; callers run them off the UI
//! thread. Nothing here keeps global state.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::qt_lock;

/// A copy saved or opened within this long is never removed by a trim.
pub const OPENING_GRACE_MS: i64 = 60_000;

const INDEX: &str = ".zerus-files.json";
const LOCK: &str = ".zerus-files.lock";
const PART: &str = ".zerus-part-";
const MAX_INDEX_BYTES: u64 = 32 * 1024 * 1024;
const LOCK_WAIT: Duration = Duration::from_secs(5);

const INDEX_INVALID: &str = "The downloaded files index is invalid. Existing files were preserved.";

/// The remote file a copy was made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub host: String,
    pub path: String,
}

/// The folder's downloaded copies, and why they could not all be counted
/// or removed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub bytes: u64,
    pub files: usize,
    pub error: Option<String>,
}

/// A copy saved into the folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completed {
    pub path: PathBuf,
    /// Why the index could not record it; the copy itself is in place.
    pub warning: Option<String>,
}

/// A download under way: a `.zerus-part-*` file in the folder that the
/// fetch writes. Dropped without [`complete`], it is removed.
#[derive(Debug)]
pub struct InProgress {
    root: PathBuf,
    canonical_root: Option<PathBuf>,
    path: PathBuf,
}

impl InProgress {
    /// Where the fetch writes the copy.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Still the folder it was made in, and still a part file of it.
    fn intact(&self) -> bool {
        fs::canonicalize(&self.root).ok() == self.canonical_root
            && self.path.parent() == Some(self.root.as_path())
            && self
                .path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(PART))
    }
}

impl Drop for InProgress {
    fn drop(&mut self) {
        if self.intact() && ordinary(&self.path) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Where copies go (DownloadCache::directory): `downloads/folder` when set,
/// otherwise `Zerus Files` in the Downloads folder, or in `~/Downloads`
/// without one. Relative paths are taken from the current folder, as Qt's
/// QFileInfo::absoluteFilePath takes them.
pub fn folder(setting: &str, downloads: Option<&Path>, home: &Path) -> PathBuf {
    let chosen = if setting.is_empty() {
        downloads
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home.join("Downloads"))
            .join("Zerus Files")
    } else {
        PathBuf::from(setting)
    };
    absolute(&chosen)
}

fn absolute(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    // Lexically clean, as QFileInfo does, without resolving links.
    let mut clean = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    clean
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}

/// A regular file of its own: not a link, a junction or a folder.
fn ordinary(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
}

/// Something, even a broken link, holds the name.
fn occupied(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// A name the index may hold: one plain name, never Zerus's own files.
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.to_lowercase().starts_with(".zerus-")
}

fn valid_source(source: &Source) -> bool {
    !source.host.is_empty()
        && !source.path.is_empty()
        && source.host.len() <= 4096
        && source.path.len() <= 65536
}

/// QDataStream (Qt 6.0) of the host and path, hashed: a QString is its
/// UTF-16 byte count (big-endian u32) and its UTF-16BE code units.
fn source_digest(source: &Source) -> String {
    let mut bytes = Vec::new();
    for text in [&source.host, &source.path] {
        let units: Vec<u16> = text.encode_utf16().collect();
        bytes.extend_from_slice(&((units.len() * 2) as u32).to_be_bytes());
        for unit in units {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
    }
    hex(&Sha256::digest(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn text<'a>(entry: &'a Map<String, Value>, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("")
}

/// QJsonValue::toInteger: a whole number, otherwise 0.
fn integer(entry: &Map<String, Value>, key: &str) -> i64 {
    match entry.get(key) {
        Some(Value::Number(number)) => number.as_i64().unwrap_or_else(|| {
            number
                .as_f64()
                .filter(|n| n.fract() == 0.0 && n.abs() < 9.2e18)
                .map_or(0, |n| n as i64)
        }),
        _ => 0,
    }
}

fn same_source(entry: &Map<String, Value>, source: &Source, digest: &str) -> bool {
    let saved = entry.get("source").and_then(Value::as_object);
    saved.is_some_and(|saved| {
        text(saved, "host") == source.host && text(saved, "path") == source.path
    }) && text(entry, "source_sha256") == digest
}

/// The name a copy is saved under (DownloadCache.cpp `filename`): the last
/// part of the remote name, with characters no file system takes replaced,
/// Windows device names and Zerus's own prefix moved aside, and at most
/// 180 UTF-8 bytes, keeping an extension of up to 32.
pub fn filename(name: &str) -> String {
    let mut name: String = name
        .rsplit('/')
        .next()
        .unwrap_or("")
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 {
                '_'
            } else {
                c
            }
        })
        .collect();
    while name.ends_with('.') || name.ends_with(' ') {
        name.pop();
    }
    if name.is_empty() || name == "." || name == ".." {
        name = "download".into();
    }
    if reserved(&name) || name.to_lowercase().starts_with(".zerus-") {
        name.insert_str(0, "download-");
    }
    let dot = name.rfind('.').filter(|dot| *dot > 0);
    let extension = dot
        .map(|dot| name[dot..].to_owned())
        .filter(|extension| extension.len() <= 32)
        .unwrap_or_default();
    let mut stem = if extension.is_empty() {
        name.clone()
    } else {
        name[..name.len() - extension.len()].to_owned()
    };
    while stem.len() + extension.len() > 180 {
        stem.pop();
    }
    stem + &extension
}

/// CON, PRN, AUX, NUL, CONIN$, CONOUT$, COM1-9, LPT1-9 (and the superscript
/// digits), alone or before a dot, in any case.
fn reserved(name: &str) -> bool {
    let upper = name.to_uppercase();
    let base = upper.split('.').next().unwrap_or("");
    let numbered = |prefix: &str| {
        base.strip_prefix(prefix).is_some_and(|rest| {
            let mut chars = rest.chars();
            matches!(chars.next(), Some('1'..='9' | '¹' | '²' | '³')) && chars.next().is_none()
        })
    };
    matches!(base, "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || numbered("COM")
        || numbered("LPT")
}

/// Makes or checks the folder: it must be a folder of its own, not a link.
fn prepare_folder(root: &Path, create: bool) -> Result<(), String> {
    let existed = occupied(root);
    if create && !existed && fs::create_dir_all(root).is_err() {
        return Err("Cannot create the downloaded files folder.".into());
    }
    if !fs::symlink_metadata(root).is_ok_and(|meta| meta.file_type().is_dir()) {
        return Err("The downloaded files folder is unavailable or is a link.".into());
    }
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(root, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

struct Snapshot {
    size: u64,
    digest: String,
}

/// The file's size and, with `hash`, its SHA-256, for the very file opened
/// at that name: unchanged while hashed, and not replaced since.
fn snapshot(path: &Path, hash: bool) -> Option<Snapshot> {
    if !ordinary(path) {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let before = file.metadata().ok()?;
    let mut digest = String::new();
    if hash {
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            let read = file.read(&mut buffer).ok()?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let after = file.metadata().ok()?;
        if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
            return None;
        }
        digest = hex(&hasher.finalize());
    }
    same_file(&file, path).then_some(Snapshot {
        size: before.len(),
        digest,
    })
}

/// DownloadCache::Detail::sameFile: the open file is the regular file at
/// `path` (not a link put there since).
fn same_file(file: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (Ok(opened), Ok(named)) = (file.metadata(), fs::symlink_metadata(path)) else {
            return false;
        };
        opened.file_type().is_file()
            && named.file_type().is_file()
            && opened.dev() == named.dev()
            && opened.ino() == named.ino()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        };
        let info = |handle: std::os::windows::io::RawHandle| {
            let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            // SAFETY: the handle is open for the call.
            (unsafe { GetFileInformationByHandle(handle as _, &mut info) } != 0).then_some(info)
        };
        let unsafe_attributes = FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT;
        let Some(before) = info(file.as_raw_handle()) else {
            return false;
        };
        let Ok(other) = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(7)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
        else {
            return false;
        };
        let Some(current) = info(other.as_raw_handle()) else {
            return false;
        };
        before.dwFileAttributes & unsafe_attributes == 0
            && current.dwFileAttributes & unsafe_attributes == 0
            && before.dwVolumeSerialNumber == current.dwVolumeSerialNumber
            && before.nFileIndexHigh == current.nFileIndexHigh
            && before.nFileIndexLow == current.nFileIndexLow
    }
}

/// A name in the folder no other file uses, made with `create_new`.
fn fresh_file(root: &Path, prefix: &str) -> io::Result<(PathBuf, File)> {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
        ^ (u128::from(std::process::id()) << 64);
    for attempt in 0u128..100 {
        let tag = hex(&Sha256::digest((seed + attempt).to_le_bytes()))[..6].to_owned();
        let path = root.join(format!("{prefix}{tag}"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("no unused name"))
}

/// The folder's index, read and held under the folder's lock.
struct Ledger {
    root: PathBuf,
    entries: Vec<Map<String, Value>>,
    _lock: qt_lock::QtLock,
}

impl Ledger {
    fn open(root: &Path) -> Result<Ledger, String> {
        let root = absolute(root);
        prepare_folder(&root, false)?;
        let busy = || "The downloaded files cache is busy.".to_owned();
        let lock_path = root.join(LOCK);
        if is_link(&lock_path) {
            return Err(busy());
        }
        let lock = qt_lock::lock(&lock_path, LOCK_WAIT)
            .ok()
            .flatten()
            .ok_or_else(busy)?;
        let index = root.join(INDEX);
        let mut ledger = Ledger {
            root,
            entries: Vec::new(),
            _lock: lock,
        };
        if !occupied(&index) {
            return Ok(ledger);
        }
        let unreadable = || "Cannot read the downloaded files index.".to_owned();
        let size = fs::symlink_metadata(&index)
            .map_err(|_| unreadable())?
            .len();
        if !ordinary(&index) || size > MAX_INDEX_BYTES {
            return Err(unreadable());
        }
        let mut bytes = Vec::new();
        File::open(&index)
            .and_then(|file| file.take(MAX_INDEX_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|_| unreadable())?;
        let document: Value =
            serde_json::from_slice(&bytes).map_err(|_| INDEX_INVALID.to_owned())?;
        let version_one = document.get("version").and_then(Value::as_f64) == Some(1.0);
        let Some(files) = document
            .get("files")
            .and_then(Value::as_array)
            .filter(|_| version_one)
        else {
            return Err(INDEX_INVALID.into());
        };
        let mut names = std::collections::HashSet::new();
        for value in files {
            let entry = value.as_object().cloned().unwrap_or_default();
            let name = text(&entry, "name");
            let sha = text(&entry, "sha256");
            let valid = safe_name(name)
                && names.insert(name.to_owned())
                && entry
                    .get("bytes")
                    .and_then(Value::as_f64)
                    .is_some_and(|bytes| bytes >= 0.0)
                && entry.get("saved").is_some_and(Value::is_number)
                && sha.len() == 64
                && sha
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
            if !valid {
                return Err(INDEX_INVALID.into());
            }
            if entry.contains_key("source") || entry.contains_key("source_sha256") {
                let saved = entry.get("source").and_then(Value::as_object);
                let identity = saved.map(|saved| Source {
                    host: text(saved, "host").to_owned(),
                    path: text(saved, "path").to_owned(),
                });
                let attributed = saved.is_some_and(|saved| saved.len() == 2)
                    && identity.as_ref().is_some_and(|identity| {
                        valid_source(identity)
                            && text(&entry, "source_sha256") == source_digest(identity)
                    });
                if !attributed {
                    return Err(INDEX_INVALID.into());
                }
            }
            ledger.entries.push(entry);
        }
        Ok(ledger)
    }

    /// Writes the index as QSaveFile does: a temporary file, then a rename.
    fn save(&self) -> Result<(), String> {
        let failed = || "Cannot save the downloaded files index.".to_owned();
        let document = json!({ "version": 1, "files": self.entries });
        let (temporary, mut file) =
            fresh_file(&self.root, ".zerus-files.json.").map_err(|_| failed())?;
        let written = file
            .write_all(document.to_string().as_bytes())
            .and_then(|()| file.sync_all());
        drop(file);
        if written.is_err() || fs::rename(&temporary, self.root.join(INDEX)).is_err() {
            let _ = fs::remove_file(&temporary);
            return Err(failed());
        }
        Ok(())
    }

    /// Ledger::clean: drops entries whose file is gone or changed size, then
    /// removes the oldest copies past the opening grace while over `limit`
    /// (0: never), each only after its content still matches.
    fn clean(&mut self, limit: u64, now: i64) -> Usage {
        let mut usage = Usage::default();
        let mut retained: Vec<Map<String, Value>> = Vec::new();
        for entry in std::mem::take(&mut self.entries) {
            let current = snapshot(&self.root.join(text(&entry, "name")), false);
            if current
                .as_ref()
                .is_none_or(|current| current.size as i64 != integer(&entry, "bytes"))
            {
                continue;
            }
            usage.bytes += current.map_or(0, |current| current.size);
            retained.push(entry);
        }
        retained.sort_by_key(|entry| integer(entry, "saved"));
        for entry in retained {
            if limit > 0
                && usage.bytes > limit
                && now - integer(&entry, "saved") >= OPENING_GRACE_MS
            {
                let path = self.root.join(text(&entry, "name"));
                let current = snapshot(&path, true);
                let bytes = integer(&entry, "bytes") as u64;
                let unchanged = current.as_ref().is_some_and(|current| {
                    current.size == bytes && current.digest == text(&entry, "sha256")
                });
                if !unchanged {
                    usage.bytes = usage.bytes.saturating_sub(bytes);
                    continue;
                }
                if fs::remove_file(&path).is_ok() {
                    usage.bytes = usage.bytes.saturating_sub(bytes);
                    continue;
                }
                usage.error = Some("Some older downloaded copies could not be removed.".into());
            }
            self.entries.push(entry);
        }
        usage.files = self.entries.len();
        usage
    }
}

/// DownloadCache::prepare: makes the folder (owner only) and a part file in
/// it for the fetch to write.
pub fn reserve(root: &Path) -> Result<InProgress, String> {
    let root = absolute(root);
    prepare_folder(&root, true)?;
    if is_link(&root.join(INDEX)) {
        return Err("The downloaded files index is a link.".into());
    }
    drop(Ledger::open(&root)?);
    let (path, file) =
        fresh_file(&root, PART).map_err(|_| "Cannot prepare the local download.".to_owned())?;
    drop(file);
    Ok(InProgress {
        canonical_root: fs::canonicalize(&root).ok(),
        root,
        path,
    })
}

/// DownloadCache::publish: the fetched copy, verified, saved under `name`
/// (with " (n)" before the extension when taken). The newest copy of a
/// source alone carries its attribution; earlier copies stay.
pub fn complete(
    part: InProgress,
    name: &str,
    source: Option<&Source>,
    now_ms: i64,
) -> Result<Completed, String> {
    let source = source.filter(|source| !source.host.is_empty() || !source.path.is_empty());
    if source.is_some_and(|source| !valid_source(source)) {
        return Err("The downloaded file source is invalid.".into());
    }
    if !part.intact() {
        return Err("The prepared download is unavailable.".into());
    }
    let data = snapshot(&part.path, true)
        .ok_or_else(|| "The downloaded copy could not be verified.".to_owned())?;
    let mut ledger = Ledger::open(&part.root)?;
    let name = filename(name);
    let dot = name.rfind('.').filter(|dot| *dot > 0);
    let (stem, extension) = match dot {
        Some(dot) => (&name[..dot], &name[dot..]),
        None => (name.as_str(), ""),
    };
    let mut destination = None;
    for number in 0..10_000 {
        let candidate = ledger.root.join(if number == 0 {
            name.clone()
        } else {
            format!("{stem} ({number}){extension}")
        });
        if occupied(&candidate) {
            continue;
        }
        // A link never replaces a name another program took meanwhile.
        match fs::hard_link(&part.path, &candidate) {
            Ok(()) => {
                let _ = fs::remove_file(&part.path);
                destination = Some(candidate);
                break;
            }
            Err(_) if occupied(&candidate) => continue,
            Err(_) => return Err("Cannot save the downloaded copy in its cache folder.".into()),
        }
    }
    let destination = destination
        .ok_or_else(|| "Cannot find an unused name for the downloaded copy.".to_owned())?;
    let mut entry = Map::new();
    entry.insert(
        "name".into(),
        json!(destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())),
    );
    entry.insert("bytes".into(), json!(data.size));
    entry.insert("sha256".into(), json!(data.digest));
    entry.insert("saved".into(), json!(now_ms));
    if let Some(source) = source {
        let digest = source_digest(source);
        for prior in &mut ledger.entries {
            if same_source(prior, source, &digest) {
                prior.remove("source");
                prior.remove("source_sha256");
            }
        }
        entry.insert(
            "source".into(),
            json!({ "host": source.host, "path": source.path }),
        );
        entry.insert("source_sha256".into(), json!(digest));
    }
    ledger.entries.push(entry);
    // Recorded before any trim: hashing older copies must not delay opening.
    ledger.clean(0, now_ms);
    let warning = ledger.save().err();
    Ok(Completed {
        path: destination,
        warning,
    })
}

/// DownloadCache::lookup: the newest copy of `source` if it is unchanged
/// (a hit renews its opening grace). A changed or missing copy loses its
/// attribution; no file is removed. A hit is a local snapshot, not proof the
/// remote file is current.
pub fn lookup(root: &Path, source: &Source, now_ms: i64) -> Result<Option<PathBuf>, String> {
    if !valid_source(source) {
        return Err("The downloaded file source is invalid.".into());
    }
    if !occupied(root) {
        return Ok(None);
    }
    let mut ledger = Ledger::open(root)?;
    let digest = source_digest(source);
    let Some(selected) = ledger
        .entries
        .iter()
        .rposition(|entry| same_source(entry, source, &digest))
    else {
        return Ok(None);
    };
    let path = ledger.root.join(text(&ledger.entries[selected], "name"));
    let current = snapshot(&path, true);
    let entry = &ledger.entries[selected];
    let valid = current.is_some_and(|current| {
        current.size as i64 == integer(entry, "bytes") && current.digest == text(entry, "sha256")
    });
    for (at, prior) in ledger.entries.iter_mut().enumerate() {
        if !same_source(prior, source, &digest) {
            continue;
        }
        if valid && at == selected {
            prior.insert("saved".into(), json!(now_ms));
        } else {
            prior.remove("source");
            prior.remove("source_sha256");
        }
    }
    ledger.save()?;
    Ok(valid.then_some(path))
}

/// DownloadCache::trim: the oldest copies removed while over `limit` bytes
/// (0: never), sparing those saved or opened within [`OPENING_GRACE_MS`].
pub fn trim(root: &Path, limit: u64, now_ms: i64) -> Usage {
    if !root.exists() {
        return Usage::default();
    }
    let mut ledger = match Ledger::open(root) {
        Ok(ledger) => ledger,
        Err(error) => {
            return Usage {
                error: Some(error),
                ..Usage::default()
            }
        }
    };
    let mut usage = ledger.clean(limit, now_ms);
    if let Err(error) = ledger.save() {
        usage.error = Some(error);
    }
    usage
}

/// DownloadCache::usage: the copies still as saved, nothing removed.
pub fn usage(root: &Path) -> Usage {
    if !root.exists() {
        return Usage::default();
    }
    match Ledger::open(root) {
        Ok(mut ledger) => ledger.clean(0, now_ms()),
        Err(error) => Usage {
            error: Some(error),
            ..Usage::default()
        },
    }
}

#[cfg(test)]
mod tests;
