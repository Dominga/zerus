//! #569: private stores repair themselves and never fail silently.
//!
//! A store whose privacy is checked (read marks, the analytics store, the
//! approval journal and the other `verify_private` callers) can be refused
//! because another tool widened its access: a sandbox installer adds a group
//! to the profile, and a file below inherits it. When the store is this user's
//! and lies in Zerus's own folders, `ensure` makes it private again in place
//! (`platform::repair_private`), notes the removed accesses in the session
//! host's log and in `private-stores.jsonl`, and the caller goes on. A store
//! that stays refused, or one a write failed on, is kept in the small current
//! set `private-stores.json`, which `hgs ls --json`, `hgs status` and
//! `hgs inspect` report with the one-line fix until the store is private again.
use crate::platform::{self, Private, PrivateRepair};
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// The stores refused now, by path: small, and the only file a listing reads.
const CURRENT: &str = "private-stores.json";
/// What was repaired or refused, newest last: owner-only and bounded.
const JOURNAL: &str = "private-stores.jsonl";
const JOURNAL_LINES: usize = 200;
const JOURNAL_BYTES: u64 = 64 * 1024;
/// The current set never grows past this many stores.
const CURRENT_LIMIT: usize = 32;

/// Whether `path` is private, after repairing it when it was this user's and
/// only too open. False leaves it refused, recorded for the reports.
pub(crate) fn ensure(path: &Path, kind: Private) -> io::Result<bool> {
    if platform::verify_private(path, kind)? {
        resolved(path);
        return Ok(true);
    }
    Ok(repair(path, kind) && platform::verify_private(path, kind)?)
}

/// `ensure` for a file the caller has open at `path`: the repair changes the
/// file itself, so the open handle is checked again.
pub(crate) fn ensure_file(path: &Path, file: &fs::File) -> io::Result<bool> {
    if platform::verify_private_file(file)? {
        resolved(path);
        return Ok(true);
    }
    Ok(repair(path, Private::File) && platform::verify_private_file(file)?)
}

/// A write to the store at `path` failed: reported until the store is next
/// found private.
pub(crate) fn write_failed(path: &Path, kind: Private, error: &dyn std::fmt::Display) {
    let reason = format!("a write failed: {error}");
    journal(json!({"path":path,"kind":kind_name(kind),"refused":reason}));
    record(path, kind, &reason, "write");
}

/// The stores refused now (path, kind, reason, fix, source, at); empty when
/// none. Reads only the small current set.
pub(crate) fn refused() -> Vec<Value> {
    stores()
}

/// The known stores of this computer, checked and repaired at a host's start
/// and by `hgs status`: a fixed list, no folder walk.
pub(crate) fn check_known() {
    for (path, kind) in known() {
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => resolved(&path),
            _ => {
                if let Err(error) = ensure(&path, kind) {
                    record(&path, kind, &format!("it could not be checked: {error}"), "check");
                }
            }
        }
    }
}

fn known() -> Vec<(PathBuf, Private)> {
    let state = state_root();
    vec![
        (state.join("read-marks.json"), Private::File),
        (state.join("analytics"), Private::Directory),
        (state.join("analytics").join("analytics.sqlite3"), Private::File),
        (state.join("approval-audit"), Private::Directory),
        (state.join("approval-audit").join("journal.sqlite3"), Private::File),
        (state.join(JOURNAL), Private::File),
    ]
}

fn state_root() -> PathBuf {
    crate::state::watch_source::state_root()
}

/// Repairs only inside Zerus's state and configuration folders.
fn within_roots(path: &Path) -> bool {
    let mut roots = vec![state_root()];
    if let Some(home) = platform::home() {
        roots.push(crate::config::config_dir(&home));
    }
    path.is_absolute() && !path.components().any(|part| part == std::path::Component::ParentDir)
        && roots.iter().any(|root| path.starts_with(root))
}

fn repair(path: &Path, kind: Private) -> bool {
    if !within_roots(path) {
        record(path, kind, "it is not private, and it is outside Zerus's folders, so Zerus does not change it", "check");
        return false;
    }
    match platform::repair_private(path, kind) {
        Ok(PrivateRepair::Private) => {
            resolved(path);
            true
        }
        Ok(PrivateRepair::Repaired(removed)) => {
            crate::host::log::note(format!("private store {} repaired; removed: {}", path.display(), removed.join(", ")));
            journal(json!({"path":path,"kind":kind_name(kind),"removed":removed}));
            resolved(path);
            true
        }
        Ok(PrivateRepair::Refused(reason)) => {
            journal(json!({"path":path,"kind":kind_name(kind),"refused":reason}));
            record(path, kind, &reason, "check");
            false
        }
        Err(error) => {
            record(path, kind, &format!("it could not be checked: {error}"), "check");
            false
        }
    }
}

fn kind_name(kind: Private) -> &'static str {
    match kind {
        Private::Directory => "folder",
        Private::File => "file",
    }
}

/// The one line that makes the store private, for the reason given.
pub(crate) fn fix(path: &Path, kind: Private, reason: &str) -> String {
    let shape = reason.contains("link") || reason.contains("not a") || reason.contains("is a folder");
    let display = path.display();
    if cfg!(windows) {
        if shape {
            return format!("move \"{display}\" \"{display}.refused\"");
        }
        let grant = if kind == Private::Directory { "%USERNAME%:(OI)(CI)F" } else { "%USERNAME%:F" };
        let icacls = format!("icacls \"{display}\" /inheritance:r /grant:r \"{grant}\"");
        if reason.contains("owned by") { format!("takeown /f \"{display}\" && {icacls}") } else { icacls }
    } else {
        let quoted = format!("'{}'", display.to_string().replace('\'', "'\\''"));
        if shape {
            return format!("mv {quoted} {quoted}.refused");
        }
        let chmod = format!("chmod {} {quoted}", if kind == Private::Directory { "700" } else { "600" });
        if reason.contains("owned by") { format!("sudo chown \"$USER\" {quoted} && {chmod}") } else { chmod }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs())
}

fn current_path() -> PathBuf {
    state_root().join(CURRENT)
}

fn load(path: &Path) -> Option<Value> {
    let mut text = String::new();
    fs::File::open(path).ok()?.take(256 * 1024).read_to_string(&mut text).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let stores = value["stores"].as_array()?.clone();
    Some(Value::Array(stores))
}

fn stores() -> Vec<Value> {
    load(&current_path()).and_then(|value| value.as_array().cloned()).unwrap_or_default()
}

fn save(stores: &[Value]) {
    let path = current_path();
    if stores.is_empty() {
        let _ = fs::remove_file(&path);
        return;
    }
    let _ = write_replacing(&path, json!({"version":1,"stores":stores}).to_string().as_bytes());
}

fn record(path: &Path, kind: Private, reason: &str, source: &str) {
    let key = path.display().to_string();
    let mut stores = stores();
    let entry = json!({"path":key,"kind":kind_name(kind),"reason":reason,"fix":fix(path, kind, reason),"source":source,"at":now()});
    if let Some(existing) = stores.iter_mut().find(|store| store["path"] == key.as_str()) {
        // The same refusal keeps its first time, so a notice is not raised again.
        if existing["reason"] == reason {
            return;
        }
        *existing = entry;
    } else {
        stores.push(entry);
        let excess = stores.len().saturating_sub(CURRENT_LIMIT);
        stores.drain(..excess);
    }
    save(&stores);
}

/// The store at `path` is private: it is no longer reported.
fn resolved(path: &Path) {
    // One stat when nothing is refused, as on every ordinary open.
    if fs::symlink_metadata(current_path()).is_err() {
        return;
    }
    let key = path.display().to_string();
    let mut stores = stores();
    let before = stores.len();
    stores.retain(|store| store["path"] != key.as_str());
    if stores.len() != before {
        save(&stores);
    }
}

/// Writes `bytes` to a new owner-only file beside `path` and renames it over.
fn write_replacing(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        platform::private_dir(parent)?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = platform::refuse_link(platform::private_open(
            fs::OpenOptions::new().create_new(true).write(true),
        ))
        .open(&temp)?;
        platform::owner_only_file(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// One line in the owner-only journal: path, kind, the removed accesses or the
/// reason, and the time. Never a store's content. Kept to the newest
/// `JOURNAL_LINES` lines and `JOURNAL_BYTES` bytes.
fn journal(mut line: Value) {
    line["at"] = json!(now());
    let path = state_root().join(JOURNAL);
    let mut text = String::new();
    if let Ok(file) = platform::refuse_link(fs::OpenOptions::new().read(true)).open(&path) {
        // The journal is a private store too; a widened one is repaired first.
        if !matches!(platform::verify_private_file(&file), Ok(true))
            && !matches!(platform::repair_private(&path, Private::File), Ok(PrivateRepair::Repaired(_) | PrivateRepair::Private))
        {
            return;
        }
        let _ = file.take(JOURNAL_BYTES + 1).read_to_string(&mut text);
    }
    let mut lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    let new = line.to_string();
    lines.push(&new);
    let mut keep = lines.len().saturating_sub(JOURNAL_LINES);
    while keep < lines.len() && lines[keep..].iter().map(|line| line.len() as u64 + 1).sum::<u64>() > JOURNAL_BYTES {
        keep += 1;
    }
    let mut out = lines[keep..].join("\n");
    out.push('\n');
    let _ = write_replacing(&path, out.as_bytes());
}

#[cfg(test)]
mod tests;
