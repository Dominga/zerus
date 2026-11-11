//! A derived native-conversation index for `hgs self`. It never proves process
//! ownership. Every candidate is reread from its authoritative binding.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    time::{Duration, Instant},
};

const CACHE_BYTES: u64 = 8192;
const CANDIDATES: usize = 8;
const METADATA_BOUND: &str = "Native identity lookup exceeded its metadata bound";

fn physical(home: &Path) -> PathBuf {
    crate::platform::canonical(home).unwrap_or_else(|_| home.to_path_buf())
}
fn path(provider: Provider, native: &str, home: &Path) -> PathBuf {
    let key = json!([provider.backend(), physical(home), native]).to_string();
    root()
        .join("native-identities")
        .join(format!("{:x}.json", Sha256::digest(key)))
}
fn cached(path: &Path) -> Option<Value> {
    let bytes = labels::read_file(path, CACHE_BYTES).ok()??;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    (value["schema"] == 1
        && value["entries"]
            .as_array()
            .is_some_and(|a| a.len() <= CANDIDATES))
    .then_some(value)
}
fn writable(path: &Path) -> bool {
    let absent = |e: io::Error| e.kind() == io::ErrorKind::NotFound;
    path.parent().is_some_and(|directory| {
        fs::symlink_metadata(directory).map_or_else(absent, |m| m.is_dir())
    }) && fs::symlink_metadata(path).map_or_else(absent, |m| m.is_file())
}
fn matches(provider: Provider, record: &Value, native: &str, home: &Path) -> bool {
    Path::new(string(record, "agent_home")).is_absolute()
        && record["backend"] == provider.backend()
        && string(record, "conversation_id") == native
        && physical(Path::new(string(record, "agent_home"))) == physical(home)
        && string(record, "archive_id").is_empty()
        && record["rename_shadow"] != true
}

/// What the provider's binding directories look like now: a write anywhere
/// in them (a new, replaced or removed binding) changes this.
fn stamp(provider: Provider) -> Value {
    let directory = |name: &str| {
        let Ok(meta) = fs::symlink_metadata(root().join(provider.backend()).join(name)) else {
            return Value::Null;
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as u64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            json!([
                modified,
                meta.dev(),
                meta.ino(),
                meta.ctime(),
                meta.ctime_nsec()
            ])
        }
        #[cfg(not(unix))]
        json!([modified])
    };
    json!([
        directory("bindings"),
        directory("sessions"),
        binding_files(provider)
    ])
}

fn unknown_binding_files() -> Value {
    json!({"unknown": uuid::Uuid::new_v4().to_string()})
}

fn binding_file(directory: &str, entry: fs::DirEntry) -> Option<(PathBuf, String)> {
    let path = entry.path();
    if path
        .extension()
        .is_some_and(|extension| extension != "json")
    {
        return None;
    }
    let file = entry.file_name().into_string().ok()?;
    Some((path, format!("{directory}/{file}")))
}

fn digest_binding_files(mut files: Vec<(PathBuf, String)>) -> Option<String> {
    files.sort_by(|left, right| left.1.cmp(&right.1));
    let mut digest = Sha256::new();
    for (path, name) in files {
        let meta = fs::symlink_metadata(&path).ok()?;
        digest.update(name.as_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            digest.update(meta.dev().to_le_bytes());
            digest.update(meta.ino().to_le_bytes());
            digest.update(meta.size().to_le_bytes());
            digest.update(meta.mtime().to_le_bytes());
            digest.update(meta.mtime_nsec().to_le_bytes());
            digest.update(meta.ctime().to_le_bytes());
            digest.update(meta.ctime_nsec().to_le_bytes());
        }
        #[cfg(not(unix))]
        {
            digest.update(meta.len().to_le_bytes());
            digest.update(
                meta.modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |elapsed| elapsed.as_nanos() as u64)
                    .to_le_bytes(),
            );
        }
    }
    Some(format!("{:x}", digest.finalize()))
}

/// The identity of every binding file, folded into one digest: an in-place
/// rewrite keeps the directory entry but changes this, so a stale index is not
/// trusted (zerus-hgt89). An entry-count overflow has a stable bounded digest,
/// used only to avoid repeated scans while lookup stays unavailable. Time,
/// entry and metadata unknowns keep a fresh marker (zerus-wult0).
fn binding_files(provider: Provider) -> Value {
    let until = Instant::now() + Duration::from_secs(2);
    let mut count = 0usize;
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    for name in ["bindings", "sessions"] {
        let Ok(entries) = fs::read_dir(root().join(provider.backend()).join(name)) else {
            continue;
        };
        for entry in entries {
            count += 1;
            if count > RECORDS {
                let Ok(entry) = entry else {
                    return unknown_binding_files();
                };
                if let Some(file) = binding_file(name, entry) {
                    files.push(file);
                }
                return digest_binding_files(files)
                    .map_or_else(unknown_binding_files, |digest| json!({"overflow": digest}));
            }
            if Instant::now() >= until {
                return unknown_binding_files();
            }
            let Ok(entry) = entry else {
                return unknown_binding_files();
            };
            if let Some(file) = binding_file(name, entry) {
                files.push(file);
            }
        }
    }
    digest_binding_files(files).map_or_else(unknown_binding_files, |digest| json!(digest))
}
/// The provider's write ledger: the directories as hgs last accounted for
/// them and when, the epoch of the complete indexes made since the last
/// complete scan, and since when writes hgs did not account for itself (an
/// older hgs, a guarded rename) may have added bindings (zerus-zww71).
fn ledger_path(provider: Provider) -> PathBuf {
    root()
        .join("native-identities")
        .join(format!("{}-writes.json", provider.backend()))
}
fn ledger(provider: Provider) -> Option<Value> {
    let bytes = labels::read_file(&ledger_path(provider), CACHE_BYTES).ok()??;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    (value["schema"] == 1 && value["epoch"].is_string() && value["at"].is_u64()).then_some(value)
}
fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}
fn write_ledger(provider: Provider, epoch: &Value, dirty_since: Option<u64>) {
    let path = ledger_path(provider);
    if writable(&path) {
        let value = json!({"schema":1,"epoch":epoch,"stamp":stamp(provider),"at":now_ns(),"dirty_since":dirty_since});
        let _ = atomic(&path, &value.to_string());
    }
}

/// Under the provider lock, remember only an unavailable overflow witness.
/// This neither cleans the write ledger nor advances its complete-index epoch.
fn record_overflow(provider: Provider) {
    let path = ledger_path(provider);
    if !writable(&path) {
        return;
    }
    let Some(mut value) = ledger(provider) else {
        return;
    };
    let stamp = stamp(provider);
    if stamp[2]["overflow"].is_string() {
        value["overflow_stamp"] = stamp;
        let _ = atomic(&path, &value.to_string());
    }
}

/// The directories before a writer that accounts for itself changes them.
pub(super) fn before_write(provider: Provider) -> Value {
    stamp(provider)
}
/// After such a write, under the provider lock. When the directories were not
/// as last accounted for before it, someone else wrote meanwhile: lookups keep
/// checking what changed since then until a complete scan.
pub(super) fn account(provider: Provider, before: &Value) {
    match ledger(provider) {
        Some(ledger) => {
            let dirty = ledger["dirty_since"].as_u64().or_else(|| {
                (ledger["stamp"] != *before).then(|| ledger["at"].as_u64().unwrap_or(0))
            });
            write_ledger(provider, &ledger["epoch"], dirty);
        }
        None => write_ledger(provider, &json!(uuid::Uuid::new_v4().to_string()), None),
    }
}
fn changed_ns(meta: &fs::Metadata) -> u64 {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as u64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        modified.max((meta.ctime().max(0) as u64) * 1_000_000_000 + meta.ctime_nsec().max(0) as u64)
    }
    #[cfg(not(unix))]
    modified
}
/// Whether a binding changed at or after `since` holds `native` for a session
/// the complete index does not list. A binding that cannot be read holds no
/// session, as for the index itself. None when the bounded check cannot
/// finish: then only a complete scan decides.
fn unlisted_holder(
    provider: Provider,
    native: &str,
    home: &Path,
    since: u64,
    listed: &[Value],
) -> Option<bool> {
    // A second of timestamp granularity and clock difference.
    let since = since.saturating_sub(1_000_000_000);
    let until = Instant::now() + Duration::from_secs(2);
    let (mut count, mut bytes) = (0, 0u64);
    for (directory, by_id) in [
        (root().join(provider.backend()).join("bindings"), false),
        (root().join(provider.backend()).join("sessions"), true),
    ] {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        for entry in entries {
            count += 1;
            if count > RECORDS || Instant::now() >= until {
                return None;
            }
            let file = entry.ok()?.path();
            if file.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(&file) else {
                continue;
            };
            if changed_ns(&meta) < since {
                continue;
            }
            bytes += meta.len();
            if bytes > 64 << 20 {
                return None;
            }
            let id = by_id.then(|| {
                file.file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default()
            });
            let Ok(record) = provider.read_at(&file, id) else {
                continue;
            };
            if matches(provider, &record, native, home)
                && !listed
                    .iter()
                    .any(|e| e["session_id"] == record["session_id"])
            {
                return Some(true);
            }
        }
    }
    Some(false)
}

/// Whether the derived index recorded this binding. Provider writers already
/// hold the provider lock. A note that cannot be written never fails the
/// binding's write, but the write stays unaccounted, so a later warm lookup
/// cannot trust the index (zerus-8g1je): `false` means this identity belongs
/// in the index and could not be written there.
pub(super) fn note(provider: Provider, record: &Value) -> bool {
    let id = string(record, "session_id");
    let native = string(record, "conversation_id");
    let home = Path::new(string(record, "agent_home"));
    if !session_id::canonical(id)
        || native.is_empty()
        || native.len() > 512
        || !home.is_absolute()
        || !matches(provider, record, native, home)
    {
        // Nothing to record: this binding never answers a native lookup.
        return true;
    }
    let path = path(provider, native, home);
    if !writable(&path) {
        return false;
    }
    let mut value = cached(&path).unwrap_or_else(|| json!({"schema":1,"entries":[]}));
    let entry = json!({"session_id":id,"name":record["name"]});
    let entries = value["entries"].as_array_mut().unwrap();
    if entries.contains(&entry) {
        return true;
    }
    entries.retain(|e| e["session_id"] != id);
    if entries.len() == CANDIDATES {
        value["overflow"] = json!(true);
    } else {
        entries.push(entry);
    }
    atomic(&path, &value.to_string()).is_ok()
}

/// Whether the derived index no longer lists this binding. `false` means an
/// entry may remain, so the removal stays unaccounted (zerus-8g1je).
pub(super) fn retire(provider: Provider, record: &Value) -> bool {
    let path = path(
        provider,
        string(record, "conversation_id"),
        Path::new(string(record, "agent_home")),
    );
    if !writable(&path) {
        return false;
    }
    let Some(mut value) = cached(&path) else {
        return true;
    };
    let entries = value["entries"].as_array_mut().unwrap();
    entries.retain(|e| e["session_id"] != record["session_id"] || e["name"] != record["name"]);
    if entries.is_empty() && value["overflow"] != true {
        fs::remove_file(path).is_ok()
    } else {
        atomic(&path, &value.to_string()).is_ok()
    }
}

fn unique(mut records: Vec<Value>) -> Result<Option<Value>> {
    if records.len() > 1 {
        return Err("The native conversation matches multiple Zerus sessions; choose an exact session identity".into());
    }
    Ok(records.pop())
}

/// The derived cache path for one identity, for the process-isolated fixtures.
#[cfg(test)]
pub(in crate::state) fn cache_path_for(provider: Provider, native: &str, home: &Path) -> PathBuf {
    path(provider, native, home)
}

#[cfg(test)]
static COMPLETE_SCANS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(in crate::state) fn complete_scan_count() -> usize {
    COMPLETE_SCANS.load(std::sync::atomic::Ordering::Relaxed)
}

pub(super) fn find(provider: Provider, native: &str, home: &Path) -> Result<Option<Value>> {
    if native.is_empty() || native.len() > 512 || native.chars().any(char::is_control) {
        return Err("Invalid native session identity".into());
    }
    let cache = path(provider, native, home);
    let accounted = ledger(provider);
    let now = stamp(provider);
    let over_bound = now[2]["overflow"].is_string();
    if over_bound
        && accounted
            .as_ref()
            .is_some_and(|ledger| ledger["stamp"] == now || ledger["overflow_stamp"] == now)
    {
        // A matching overflow avoids work only; it never supplies an identity.
        return Err(METADATA_BOUND.into());
    }
    let dirty = accounted
        .as_ref()
        .is_some_and(|ledger| ledger["stamp"] != now || ledger["dirty_since"].is_u64());
    if dirty {
        // A complete scan re-baselines a ledger that fell behind an
        // unaccounted write, instead of making every warm claim pay the
        // bounded changed-binding check (zerus-8g1je). A scan that cannot
        // complete (a busy lock, a bound, or unrelated metadata it refuses)
        // leaves the answer to the warm index and that check, as before.
        match complete_scan(provider, native, home) {
            Ok(Scan::Done(record)) => return Ok(record),
            Ok(Scan::Unavailable(reason)) if over_bound && reason == METADATA_BOUND => {
                // Its stable witness lets the next lookup avoid this scan.
                return Err(reason);
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    let warm = cached(&cache).filter(|value| {
        let Some(ledger) = &accounted else {
            return false;
        };
        if value["complete"] != true || value["epoch"] != ledger["epoch"] {
            return false;
        }
        // Writes hgs did not account for: trust the index only when none of
        // the bindings changed since then holds this identity unlisted.
        let since = dirty.then(|| {
            ledger["dirty_since"]
                .as_u64()
                .unwrap_or(ledger["at"].as_u64().unwrap_or(0))
        });
        since.is_none_or(|since| {
            unlisted_holder(
                provider,
                native,
                home,
                since,
                value["entries"].as_array().map_or(&[], Vec::as_slice),
            ) == Some(false)
        })
    });
    if let Some(value) = warm {
        if value["overflow"] == true {
            return Err(
                "The native identity index is ambiguous; choose an exact session identity".into(),
            );
        }
        let mut records = Vec::new();
        for entry in value["entries"].as_array().unwrap() {
            let id = string(entry, "session_id");
            if !session_id::canonical(id) {
                return Err("The native identity index is invalid".into());
            }
            if let Some(record) = parent_link::read_session(id, string(entry, "name"))? {
                if matches(provider, &record, native, home) {
                    records.push(record);
                }
            }
        }
        if !records.is_empty() {
            return unique(records);
        }
    }
    match complete_scan(provider, native, home)? {
        Scan::Done(record) => Ok(record),
        Scan::Unavailable(reason) => Err(reason),
    }
}

enum Scan {
    Done(Option<Value>),
    Unavailable(String),
}

/// Legacy bindings predate this derived index. A complete, bounded metadata
/// scan can seed it; partial coverage never chooses a caller. A busy lock or an
/// exceeded bound is reported as unavailable, so a warm lookup can answer; any
/// other error is definitive for a cold lookup.
fn complete_scan(provider: Provider, native: &str, home: &Path) -> Result<Scan> {
    #[cfg(test)]
    COMPLETE_SCANS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let Some(_writer) = lock_until(
        &root().join(provider.backend()).join("bindings.lock"),
        Instant::now() + Duration::from_millis(100),
    )?
    else {
        return Ok(Scan::Unavailable(
            "Native session metadata is busy; retry hgs self".into(),
        ));
    };
    let until = Instant::now() + Duration::from_secs(2);
    let mut count = 0;
    let mut bytes = 0;
    let mut records = Vec::new();
    for (directory, by_id) in [
        (root().join(provider.backend()).join("bindings"), false),
        (root().join(provider.backend()).join("sessions"), true),
    ] {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        for entry in entries {
            count += 1;
            if count > RECORDS {
                record_overflow(provider);
                return Ok(Scan::Unavailable(METADATA_BOUND.into()));
            }
            if Instant::now() >= until {
                return Ok(Scan::Unavailable(METADATA_BOUND.into()));
            }
            let file = entry.map_err(|e| e.to_string())?.path();
            if file.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let raw = labels::read_file(&file, BYTES)
                .map_err(|e| e.to_string())?
                .ok_or("A native binding disappeared")?;
            bytes += raw.len();
            if bytes > 64 << 20 {
                return Ok(Scan::Unavailable(
                    "Native identity lookup exceeded its byte bound".into(),
                ));
            }
            let id = by_id.then(|| {
                file.file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default()
            });
            let record = provider.read_at(&file, id)?;
            if !by_id && provider.label_path(string(&record, "name")) != file {
                return Err("Native binding location changed".into());
            }
            if matches(provider, &record, native, home) {
                if !session_id::canonical(string(&record, "session_id")) {
                    return Err("The native session has no durable Zerus identity yet".into());
                }
                records.push(record);
            }
        }
    }
    if Instant::now() >= until {
        return Ok(Scan::Unavailable(
            "Native identity lookup exceeded its metadata bound".into(),
        ));
    }
    let record = unique(records)?;
    if let Some(record) = &record {
        // A writer's optional note alone cannot establish complete coverage:
        // other legacy bindings may not have been indexed yet. Only this
        // complete scan under the provider lock can establish a warm index.
        // The scan read the directories as they are now, under the lock:
        // they become the accounted state, in a new epoch after any foreign write.
        if writable(&ledger_path(provider)) && writable(&path(provider, native, home)) {
            // Unaccounted writes may have touched other identities too: a new
            // epoch retires every older index, which then scans again.
            let clean = ledger(provider)
                .filter(|l| l["stamp"] == stamp(provider) && l["dirty_since"].is_null());
            let epoch = clean.map_or_else(
                || json!(uuid::Uuid::new_v4().to_string()),
                |l| l["epoch"].clone(),
            );
            write_ledger(provider, &epoch, None);
            if ledger(provider).is_some_and(|l| l["epoch"] == epoch) {
                let value = json!({"schema":1,"complete":true,"epoch":epoch,"entries":[{
                    "session_id":record["session_id"],"name":record["name"]}]});
                let _ = atomic(&path(provider, native, home), &value.to_string());
            }
        }
    }
    Ok(Scan::Done(record))
}
