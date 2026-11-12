//! The session-ID index (zerus-ucck P2a): `ids/<session_id>.json` names the
//! label and the kind of record of the live or saved session that carries the
//! ID, so `--session-id` and the agent's own hooks find it without reading
//! every record.
//!
//! It is a verified cache, never a source of truth. Records stay where they
//! are (`sha256(label)`, `bindings/`, `name_routes`, the DeepSeek and OpenCode
//! bindings), so an older hgs, which ignores `ids/`, loses nothing. Every hit
//! reads that record again and needs that session ID, no archive and no
//! rename shadow; a miss or a stale entry falls back to one scan, and the scan
//! repairs the entry under the state writer lock when it is free. Writers note
//! the entry under whatever lock already guards their record; a failed note
//! only costs the next lookup a scan. Hooks never repair: their poll loops
//! must not take the writer lock. A process that finds `ids/` or an entry it
//! must not write, or fails to change one, stops changing entries, so a
//! broken index costs each lookup one scan and never the lock (#1400 review).
use super::*;
use std::fs;
use std::io::{self, Read};
#[cfg(test)]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};

/// Entries are tiny; anything larger is not one of ours.
const ENTRY_BYTES: u64 = 4096;

/// Where a session's record lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// A session record at its label's path (`storage::read`).
    Record,
    /// A session record in the ID layout, `sessions/<id>.json` (zerus-ucck
    /// P3a, `storage::read_id`).
    IdRecord,
    /// A DeepSeek binding (`dsh::binding`).
    DeepSeek,
    DeepSeekId,
    /// An OpenCode binding (`opencode::binding`).
    OpenCode,
    OpenCodeId,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Record => "record",
            Kind::IdRecord => "id",
            Kind::DeepSeek => "dsh",
            Kind::DeepSeekId => "dsh-id",
            Kind::OpenCode => "opencode",
            Kind::OpenCodeId => "opencode-id",
        }
    }
    fn parse(text: &str) -> Option<Kind> {
        match text {
            "record" => Some(Kind::Record),
            "id" => Some(Kind::IdRecord),
            "dsh" => Some(Kind::DeepSeek),
            "dsh-id" => Some(Kind::DeepSeekId),
            "opencode" => Some(Kind::OpenCode),
            "opencode-id" => Some(Kind::OpenCodeId),
            _ => None,
        }
    }
}

/// A verified hit: the session's current label, its kind and its record.
pub(super) struct Found {
    pub name: String,
    pub kind: Kind,
    pub record: Value,
}

fn directory() -> PathBuf {
    root().join("ids")
}

fn path(id: &str) -> PathBuf {
    directory().join(format!("{id}.json"))
}

/// When this process last found `ids/` or an entry it must not write, or
/// failed to change an entry, in milliseconds since the epoch; 0 for never.
/// It changes no entry for [`RETRY_MS`] after that, then checks again, so a
/// transient failure (a full disk) in a long-lived writer, such as a
/// supervisor or the desktop service, pauses its notes instead of ending them
/// (zerus-2sd29). A check needs no lock, so a hook never takes one.
static TRIPPED: AtomicU64 = AtomicU64::new(0);
/// How long a tripped latch holds.
const RETRY_MS: u64 = 60_000;
/// Moves the latch's clock forward (tests only).
#[cfg(test)]
static LATER_MS: AtomicU64 = AtomicU64::new(0);
/// Makes `ids/` look owned by another user (tests only).
#[cfg(all(test, unix))]
static FOREIGN: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    #[cfg(test)]
    let now = now + LATER_MS.load(Ordering::SeqCst);
    now.max(1)
}

fn trip() {
    TRIPPED.store(now_ms(), Ordering::Relaxed);
}

/// Whether the latch holds: tripped less than [`RETRY_MS`] ago. A clock that
/// moved back before the trip ends it, so it is never held longer.
fn tripped() -> bool {
    let at = TRIPPED.load(Ordering::Relaxed);
    let now = now_ms();
    at != 0 && now >= at && now - at < RETRY_MS
}

/// Whether this process can write in the directory `meta` describes: on
/// Unix, its owner is the effective user and may write (#1400 review: mode
/// bits alone let a foreign-owned 0755 `ids/` cost a lock and a write).
#[cfg(unix)]
fn may_write(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    #[cfg(test)]
    if FOREIGN.load(Ordering::SeqCst) {
        return false;
    }
    // SAFETY: geteuid has no preconditions.
    meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o200 != 0
}

#[cfg(not(unix))]
fn may_write(meta: &fs::Metadata) -> bool {
    !meta.permissions().readonly()
}

/// Whether this process may change `id`'s entry: the latch does not hold,
/// `ids/` is absent or a directory it may write, and the entry is absent or a
/// regular file. Two metadata reads, never the lock.
fn writable(id: &str) -> bool {
    if tripped() {
        return false;
    }
    let absent = |error: io::Error| error.kind() == io::ErrorKind::NotFound;
    let directory = fs::symlink_metadata(directory())
        .map_or_else(absent, |meta| meta.is_dir() && may_write(&meta));
    let ok = directory && fs::symlink_metadata(path(id)).map_or_else(absent, |meta| meta.is_file());
    if !ok {
        trip();
    }
    ok
}

/// Records a failed change, so this process makes none for a while.
fn changed(result: Result<()>) {
    if result.is_err() {
        trip();
    }
}

/// The entry for `id`, when there is a well-formed one in a regular file.
/// Opened without blocking, so a FIFO there is a miss, never a hang.
fn entry(id: &str) -> Option<(String, Kind)> {
    if !session_id::canonical(id) {
        return None;
    }
    let file = crate::platform::never_block(fs::OpenOptions::new().read(true))
        .open(path(id))
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(ENTRY_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > ENTRY_BYTES {
        return None;
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let name = string(&value, "name");
    (value["version"] == 1 && !name.is_empty())
        .then(|| Kind::parse(string(&value, "kind")).map(|kind| (name.to_owned(), kind)))
        .flatten()
}

fn write_entry(id: &str, name: &str, kind: Kind) -> Result<()> {
    #[cfg(test)]
    {
        CHANGES.fetch_add(1, Ordering::SeqCst);
        if FAIL_CHANGES.load(Ordering::SeqCst) {
            return Err("injected failure".into());
        }
    }
    let value = json!({"version": 1, "name": name, "kind": kind.as_str()});
    storage::atomic(&path(id), &format!("{value}\n"))
}

fn remove_entry(id: &str) -> Result<()> {
    #[cfg(test)]
    CHANGES.fetch_add(1, Ordering::SeqCst);
    storage::cut_write()?;
    match fs::remove_file(path(id)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.to_string()),
        _ => Ok(()),
    }
}

/// Notes `record`, just saved by a writer of `kind`, under its label. Only a
/// live or saved record with a session ID is noted, and an unchanged entry is
/// not written again. Never fails the caller's write.
pub(super) fn note(record: &Value, kind: Kind) {
    let (id, name) = (string(record, "session_id"), string(record, "name"));
    if !session_id::canonical(id)
        || name.is_empty()
        || !string(record, "archive_id").is_empty()
        || record["rename_shadow"] == true
    {
        return;
    }
    if entry(id).is_some_and(|(noted, noted_kind)| noted == name && noted_kind == kind)
        || !writable(id)
    {
        return;
    }
    changed(write_entry(id, name, kind));
}

/// The same optional index policy under the original recovery owner. Its
/// concrete JSON write goes through Effects, and a refused selected effect
/// propagates as well as tripping this process's existing write-error latch.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn note_guarded(
    view: &mut guarded_recovery::View<'_>,
    record: &Value,
    kind: Kind,
) -> Result<()> {
    view.check()?;
    let (id, name) = (string(record, "session_id"), string(record, "name"));
    if !session_id::canonical(id)
        || name.is_empty()
        || !string(record, "archive_id").is_empty()
        || record["rename_shadow"] == true
    {
        return Ok(());
    }
    if !writable(id) {
        return view.check();
    }
    let previous = view
        .read(&path(id), ENTRY_BYTES)?
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if previous.as_ref().is_some_and(|entry| {
        entry["version"] == 1
            && string(entry, "name") == name
            && Kind::parse(string(entry, "kind")) == Some(kind)
    }) {
        return view.check();
    }
    #[cfg(test)]
    {
        CHANGES.fetch_add(1, Ordering::SeqCst);
        if FAIL_CHANGES.load(Ordering::SeqCst) {
            trip();
            return Err("injected failure".into());
        }
    }
    let value = json!({"version":1,"name":name,"kind":kind.as_str()});
    let result = view.atomic(&path(id), format!("{value}\n").as_bytes());
    if result.is_err() {
        trip();
    }
    result
}

/// Removes `record`'s entry while it still names that label as a record: the
/// archive or forget that just retired the label, under the writer lock it
/// holds, so `ids/` does not keep an entry for every session ever made.
pub(super) fn retire(record: &Value) {
    let (id, name) = (string(record, "session_id"), string(record, "name"));
    if entry(id).is_some_and(|(noted, kind)| {
        noted == name && matches!(kind, Kind::Record | Kind::IdRecord)
    }) && writable(id)
    {
        changed(remove_entry(id));
    }
}

/// Whether `record`, read as `name`, is the live or saved session `id`.
fn carries(record: &Value, id: &str, name: &str) -> bool {
    string(record, "session_id") == id
        && string(record, "name") == name
        && string(record, "archive_id").is_empty()
        && record["rename_shadow"] != true
}

/// A provider retires only its own exact cached route, under its binding lock.
pub(super) fn retire_provider(record: &Value, kind: Kind) {
    let (id, name) = (string(record, "session_id"), string(record, "name"));
    if entry(id).is_some_and(|(found, route)| found == name && route == kind) && writable(id) {
        changed(remove_entry(id));
    }
}

/// The index entry alone, read without blocking and never following a record:
/// the label and kind it names, unverified (zerus-mzgfh).
pub(super) fn indexed(id: &str) -> Option<(String, Kind)> {
    entry(id)
}

pub(super) fn find(id: &str) -> Option<Found> {
    let (name, kind) = entry(id)?;
    let record = match kind {
        Kind::Record => storage::label_record_path(&name)
            .exists()
            .then(|| storage::read_label(&name).ok())
            .flatten()?,
        Kind::IdRecord => storage::read_id(id).ok()?,
        Kind::DeepSeek => provider_binding::Provider::DeepSeek.read_label(&name).ok()?,
        Kind::DeepSeekId => dsh::read_id(id).ok()?,
        Kind::OpenCode => provider_binding::Provider::OpenCode.read_label(&name).ok()?,
        Kind::OpenCodeId => opencode::read_id(id).ok()?,
    };
    carries(&record, id, &name).then_some(Found { name, kind, record })
}

/// Same fixed route proof without presentation projections or index repair.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn find_metadata(id: &str) -> Option<Found> {
    let (name, kind) = entry(id)?;
    let record = fixed_metadata(&name, kind, id).ok()?;
    carries(&record, id, &name).then_some(Found { name, kind, record })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn fixed_metadata(name: &str, kind: Kind, id: &str) -> Result<Value> {
    use provider_binding::Provider;
    match kind {
        Kind::Record | Kind::IdRecord => {
            let path = if kind == Kind::IdRecord {
                storage::id_record_path(id).ok_or("Invalid metadata session ID")?
            } else {
                storage::label_record_path(name)
            };
            let raw = labels::read_file(&path, storage::ID_RECORD_BYTES)
                .map_err(|e| e.to_string())?
                .ok_or("Metadata binding is absent")?;
            storage::verify_metadata_at(&path, name, &raw)
        }
        Kind::DeepSeek => Provider::DeepSeek.read_label(name),
        Kind::DeepSeekId => Provider::DeepSeek.read_id(id),
        Kind::OpenCode => Provider::OpenCode.read_label(name),
        Kind::OpenCodeId => Provider::OpenCode.read_id(id),
    }
}

/// Keep the actual validated route with the lookup result; a vanished ID file
/// must never turn a previously selected ID route into a label-holder lookup.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn metadata_records_for_id_unrepaired(id: &str) -> Result<Vec<Value>> {
    if !session_id::canonical(id) {
        return Err("A session ID must be a canonical UUID".into());
    }
    if let Some(found) = find_metadata(id) {
        return Ok(vec![found.record]);
    }
    scanned();
    scan_metadata(Some(id))
}

/// Active authoritative metadata for an old reference without a SID. The
/// caller still matches its exact conversation/run; archived references are
/// refused by Effort before this route. No cache, telemetry or Root repair.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn metadata_records_unrepaired() -> Result<Vec<Value>> {
    scanned();
    scan_metadata(None)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn scan_metadata(wanted: Option<&str>) -> Result<Vec<Value>> {
    use std::time::{Duration, Instant};
    let until = Instant::now() + Duration::from_secs(2);
    let mut count = 0usize;
    let mut bytes = 0usize;
    let mut records = Vec::new();
    for namespace in storage::BINDING_DIRS {
        let folder = root().join(namespace);
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        for entry in entries {
            if count >= 5000 || Instant::now() >= until {
                return Err(
                    "The metadata ID lookup exceeded its bound; no route was selected".into(),
                );
            }
            count += 1;
            let file = entry.map_err(|e| e.to_string())?.path();
            let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if file.extension().is_none_or(|s| s != "json")
                || !storage::binding_stem(namespace, stem)
            {
                continue;
            }
            let raw = labels::read_file(&file, storage::ID_RECORD_BYTES)
                .map_err(|e| e.to_string())?
                .ok_or("A metadata binding disappeared during lookup")?;
            bytes = bytes
                .checked_add(raw.len())
                .ok_or("Metadata lookup bytes overflow")?;
            if bytes > 64 << 20 {
                return Err("The metadata ID lookup exceeded its byte bound".into());
            }
            let value: Value = serde_json::from_slice(&raw)
                .map_err(|e| format!("A metadata binding could not be read: {e}"))?;
            if wanted.is_some_and(|id| string(&value, "session_id") != id)
                || !string(&value, "archive_id").is_empty()
                || value["rename_shadow"] == true
            {
                continue;
            }
            let name = string(&value, "name");
            let id = string(&value, "session_id");
            let kind = match namespace {
                "" | "bindings" => Kind::Record,
                "sessions" => Kind::IdRecord,
                "dsh/bindings" => Kind::DeepSeek,
                "dsh/sessions" => Kind::DeepSeekId,
                "opencode/bindings" => Kind::OpenCode,
                "opencode/sessions" => Kind::OpenCodeId,
                _ => return Err("Unknown fixed metadata namespace".into()),
            };
            let record = fixed_metadata(name, kind, id)?;
            if carries(&record, id, name) {
                records.push(record);
            }
        }
    }
    if Instant::now() >= until {
        return Err("The metadata ID lookup exceeded its original bound".into());
    }
    Ok(records)
}

/// After a scan for `id` found `found` (label and kind, at most one label for
/// a sound state): points the entry at it, or removes a stale entry when the
/// scan found none. Under the state writer lock, only when it is free right
/// now: a held lock, which may be this command's own, skips the repair. Only
/// `names_of` repairs, never a hook; an entry this process may not change
/// ([`writable`]) is never attempted.
pub(super) fn repair(id: &str, found: &[(String, Kind)]) {
    if !session_id::canonical(id) {
        return;
    }
    let mut names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    let current = entry(id);
    let wanted = match names.as_slice() {
        [] => None,
        [name] => found
            .iter()
            .filter(|(found, _)| found == name)
            .map(|(_, kind)| *kind)
            .min_by_key(|kind| *kind != Kind::Record)
            .map(|kind| ((*name).to_owned(), kind)),
        _ => return,
    };
    if current == wanted || (wanted.is_none() && !path(id).exists()) || !writable(id) {
        return;
    }
    #[cfg(test)]
    LOCKS.fetch_add(1, Ordering::SeqCst);
    let Ok(Some(_guard)) = storage::writer_until(std::time::Instant::now()) else {
        return;
    };
    changed(match wanted {
        Some((name, kind)) => write_entry(id, &name, kind),
        None => remove_entry(id),
    });
}

/// How many full scans the lookups made, for the bound tests.
#[cfg(test)]
pub(super) static SCANS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// How many times a repair asked for the writer lock (tests only).
#[cfg(test)]
static LOCKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// How many entry writes and removals were attempted (tests only).
#[cfg(test)]
static CHANGES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Makes every entry write fail, as a read-only mount or a full disk would,
/// which no metadata check predicts (tests only).
#[cfg(test)]
static FAIL_CHANGES: AtomicBool = AtomicBool::new(false);

/// Counts one full scan (tests only).
pub(super) fn scanned() {
    #[cfg(test)]
    SCANS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// The fixture needs Unix permissions, a FIFO and no tmux server.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::sync::atomic::Ordering;

    /// Runs the ignored fixture in a child with a private state root and no
    /// tmux server, so nothing here touches real sessions.
    #[test]
    fn the_session_id_index_is_a_verified_cache() {
        let state = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "state::session_index::tests::session_index_fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HGS_STATE_DIR", state.path().join("sessions"))
            .env("TMUX_TMPDIR", state.path().join("tmux"))
            .env("ZERUS_SESSION_INDEX_FIXTURE", "1")
            .env_remove("TMUX")
            .env_remove("HGS_SESSION")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_RUN_ID")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("test result: ok. 1 passed"),
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn scans() -> usize {
        SCANS.load(Ordering::SeqCst)
    }

    fn claude(name: &str, run: &str) -> Value {
        json!({"version":1,"agent":"claude","name":name,"conversation_id":format!("conversation-{run}"),"run_id":run,"created":100})
    }

    fn saved(name: &str, run: &str) -> (Value, String) {
        let mut record = claude(name, run);
        storage::write(&mut record).unwrap();
        let id = string(&record, "session_id").to_owned();
        (record, id)
    }

    /// One lookup, and whether it scanned.
    fn lookup(id: &str) -> (Vec<String>, bool) {
        let before = scans();
        let names = session_id::names_of(id).unwrap();
        (names, scans() > before)
    }

    #[test]
    #[ignore = "run by the_session_id_index_is_a_verified_cache"]
    fn session_index_fixture() {
        if std::env::var_os("ZERUS_SESSION_INDEX_FIXTURE").is_none() {
            return;
        }
        std::fs::create_dir_all(root()).unwrap();
        let a = "claude/example/a";

        // A writer notes the entry; a hit verifies it and never scans.
        let (record, id) = saved(a, "run-a");
        assert_eq!(entry(&id), Some((a.to_owned(), Kind::Record)));
        assert_eq!(lookup(&id), (vec![a.to_owned()], false));
        let modified = std::fs::metadata(path(&id)).unwrap().modified().unwrap();
        let mut again = record.clone();
        storage::write(&mut again).unwrap();
        assert_eq!(
            std::fs::metadata(path(&id)).unwrap().modified().unwrap(),
            modified,
            "an unchanged entry is not rewritten"
        );

        // A missing entry costs one scan, which repairs it.
        std::fs::remove_file(path(&id)).unwrap();
        assert_eq!(lookup(&id), (vec![a.to_owned()], true));
        assert_eq!(lookup(&id), (vec![a.to_owned()], false));

        // A malformed or foreign entry is a miss, never trusted.
        for text in [
            "not json",
            r#"{"version":2,"name":"claude/example/a","kind":"record"}"#,
            r#"{"version":1,"name":"claude/example/a","kind":"tmux"}"#,
            r#"{"version":1,"name":"","kind":"record"}"#,
        ] {
            std::fs::write(path(&id), text).unwrap();
            assert_eq!(lookup(&id), (vec![a.to_owned()], true), "{text}");
        }
        std::fs::write(path(&id), "x".repeat(ENTRY_BYTES as usize + 1)).unwrap();
        assert_eq!(lookup(&id), (vec![a.to_owned()], true), "oversized");

        // An entry naming another session's label does not verify.
        let (_, other_id) = saved("claude/example/other", "run-other");
        write_entry(&id, "claude/example/other", Kind::Record).unwrap();
        assert_eq!(lookup(&id), (vec![a.to_owned()], true));
        assert_eq!(
            lookup(&other_id),
            (vec!["claude/example/other".to_owned()], false)
        );

        // A rename moves the entry with the record; the old label's shadow and
        // a stale entry pointing at it never verify.
        let b = "claude/example/b";
        assert_eq!(
            rename::dispatch(&[a.to_owned(), b.to_owned()], false).unwrap(),
            0
        );
        assert_eq!(lookup(&id), (vec![b.to_owned()], false));
        let mut shadow = storage::read(b).unwrap();
        shadow["name"] = json!(a);
        shadow["rename_target"] = json!(b);
        shadow["rename_shadow"] = json!(true);
        storage::atomic(&storage::legacy_record_path(a), &format!("{shadow}\n")).unwrap();
        write_entry(&id, a, Kind::Record).unwrap();
        assert_eq!(lookup(&id), (vec![b.to_owned()], true));
        assert_eq!(entry(&id), Some((b.to_owned(), Kind::Record)), "repaired");
        std::fs::remove_file(storage::legacy_record_path(a)).unwrap();

        // Archive and forget retire the entry under their writer lock.
        let (retired, retired_id) = saved("claude/example/retired", "run-retired");
        {
            let _guard = storage::lock(None).unwrap();
            archive::save(&retired).unwrap();
        }
        assert!(!path(&retired_id).exists(), "archive retires the entry");
        assert_eq!(lookup(&retired_id), (vec![], true));
        let (_, forgotten_id) = saved("claude/example/forget", "run-forget");
        crate::state::dispatch(&["forget".to_owned(), "claude/example/forget".to_owned()]).unwrap();
        assert!(!path(&forgotten_id).exists(), "forget retires the entry");

        // A record removed some other way: the entry goes stale, one scan
        // finds nothing and removes it.
        let (mut archived, archived_id) = saved("claude/example/archived", "run-archived");
        archived["archive_id"] = json!("archive-1");
        storage::atomic_record(
            &root().join("archives").join("archive-1.json"),
            &mut archived,
        )
        .unwrap();
        std::fs::remove_file(storage::record_path("claude/example/archived")).unwrap();
        assert_eq!(lookup(&archived_id), (vec![], true));
        assert!(!path(&archived_id).exists(), "a stale entry is removed");
        assert_eq!(
            lookup(&archived_id),
            (vec![], true),
            "a miss without an entry scans once more and writes nothing"
        );
        assert!(!path(&archived_id).exists());
        let (_, forgotten_id) = saved("claude/example/forgotten", "run-forgotten");
        std::fs::remove_file(storage::record_path("claude/example/forgotten")).unwrap();
        assert_eq!(lookup(&forgotten_id), (vec![], true));
        assert!(!path(&forgotten_id).exists());

        // An older hgs rebuilds the record without its ID: the entry no longer
        // verifies, and the next write here notes the new ID once.
        let (mut older, older_id) = saved("claude/example/older", "run-older");
        older.as_object_mut().unwrap().remove("session_id");
        std::fs::write(
            storage::record_path("claude/example/older"),
            format!("{older}\n"),
        )
        .unwrap();
        assert_eq!(lookup(&older_id), (vec![], true));
        assert!(!path(&older_id).exists());
        storage::write(&mut older).unwrap();
        let reminted = string(&older, "session_id").to_owned();
        assert_ne!(reminted, older_id);
        assert_eq!(
            lookup(&reminted),
            (vec!["claude/example/older".to_owned()], false)
        );

        // DeepSeek and OpenCode bindings are noted by their own writers.
        let mut deepseek = json!({"backend":"dsh","agent":"dsh","name":"dsh/example/one","conversation_id":"session-1","run_id":"run-dsh"});
        dsh::save(&mut deepseek).unwrap();
        let deepseek_id = string(&deepseek, "session_id").to_owned();
        assert_eq!(
            entry(&deepseek_id),
            Some(("dsh/example/one".to_owned(), Kind::DeepSeek))
        );
        assert_eq!(
            lookup(&deepseek_id),
            (vec!["dsh/example/one".to_owned()], false)
        );
        let mut opencode = json!({"backend":"opencode","agent":"opencode","name":"opencode/example/one","conversation_id":"ses_1","run_id":"run-opencode"});
        opencode::save(&mut opencode).unwrap();
        let opencode_id = string(&opencode, "session_id").to_owned();
        assert_eq!(
            lookup(&opencode_id),
            (vec!["opencode/example/one".to_owned()], false)
        );
        std::fs::remove_file(path(&opencode_id)).unwrap();
        assert_eq!(
            lookup(&opencode_id),
            (vec!["opencode/example/one".to_owned()], true)
        );
        assert_eq!(
            entry(&opencode_id),
            Some(("opencode/example/one".to_owned(), Kind::OpenCode)),
            "repaired with its kind"
        );

        // A held writer lock, possibly this command's own, skips the repair.
        std::fs::remove_file(path(&reminted)).unwrap();
        {
            let _held = storage::lock(None).unwrap();
            let started = std::time::Instant::now();
            assert_eq!(
                lookup(&reminted),
                (vec!["claude/example/older".to_owned()], true)
            );
            assert!(
                started.elapsed() < std::time::Duration::from_millis(500),
                "a held lock is tried once, never waited for"
            );
            assert!(
                !path(&reminted).exists(),
                "no repair while the lock is held"
            );
        }
        assert_eq!(
            lookup(&reminted),
            (vec!["claude/example/older".to_owned()], true)
        );
        assert!(path(&reminted).exists());

        // Concurrent writers each note their own entry.
        let written: Vec<(String, String)> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..32)
                .map(|n| {
                    scope.spawn(move || {
                        let name = format!("claude/example/concurrent-{n}");
                        let (_, id) = saved(&name, &format!("run-concurrent-{n}"));
                        (name, id)
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect()
        });
        for (name, id) in &written {
            assert_eq!(lookup(id), (vec![name.clone()], false));
        }

        // The agent's own hooks resolve HGS_SESSION_ID through the index,
        // under the label the session carries now.
        let (_, hook_id) = saved("claude/example/hooked", "run-hooked");
        assert_eq!(
            rename::dispatch(
                &[
                    "claude/example/hooked".to_owned(),
                    "claude/example/hooked-2".to_owned()
                ],
                false
            )
            .unwrap(),
            0
        );
        std::env::set_var("HGS_SESSION_ID", &hook_id);
        let own = |run: &str| {
            let before = scans();
            let record = storage::read_own_run("claude/example/hooked", run).unwrap();
            (
                record.map(|record| string(&record, "name").to_owned()),
                scans() > before,
            )
        };
        assert_eq!(
            own("run-hooked"),
            (Some("claude/example/hooked-2".to_owned()), false)
        );
        assert_eq!(
            own("run-nobody"),
            (None, true),
            "another run is not this record"
        );
        std::fs::remove_file(path(&hook_id)).unwrap();
        // A hook never repairs and never asks for the writer lock; the
        // record's next write notes the entry again.
        let locks = LOCKS.load(Ordering::SeqCst);
        for _ in 0..3 {
            assert_eq!(
                own("run-hooked"),
                (Some("claude/example/hooked-2".to_owned()), true)
            );
        }
        assert_eq!(
            LOCKS.load(Ordering::SeqCst),
            locks,
            "a hook asks for no lock"
        );
        assert!(!path(&hook_id).exists(), "a hook never repairs");
        let mut hooked = storage::read("claude/example/hooked-2").unwrap();
        storage::write(&mut hooked).unwrap();
        assert_eq!(
            own("run-hooked"),
            (Some("claude/example/hooked-2".to_owned()), false)
        );
        std::env::remove_var("HGS_SESSION_ID");

        // A write that fails where metadata could not tell (a read-only mount,
        // a full disk) is attempted once; this process then changes no entry.
        let (_, failing_id) = saved("claude/example/failing", "run-failing");
        std::fs::remove_file(path(&failing_id)).unwrap();
        FAIL_CHANGES.store(true, Ordering::SeqCst);
        let before = (LOCKS.load(Ordering::SeqCst), CHANGES.load(Ordering::SeqCst));
        for _ in 0..20 {
            assert_eq!(
                lookup(&failing_id),
                (vec!["claude/example/failing".to_owned()], true)
            );
        }
        let after = (LOCKS.load(Ordering::SeqCst), CHANGES.load(Ordering::SeqCst));
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            (1, 1),
            "one attempt, then none"
        );
        // zerus-2sd29: the latch holds for RETRY_MS, then allows one more
        // attempt per interval while writes still fail.
        LATER_MS.fetch_add(RETRY_MS, Ordering::SeqCst);
        for _ in 0..20 {
            lookup(&failing_id);
        }
        let again = (LOCKS.load(Ordering::SeqCst), CHANGES.load(Ordering::SeqCst));
        assert_eq!(
            (again.0 - after.0, again.1 - after.1),
            (1, 1),
            "one more attempt"
        );
        // A transient failure heals: after the next interval the repair
        // succeeds, and the lookup after it is a hit.
        FAIL_CHANGES.store(false, Ordering::SeqCst);
        assert!(lookup(&failing_id).1, "still held");
        LATER_MS.fetch_add(RETRY_MS, Ordering::SeqCst);
        assert!(lookup(&failing_id).1, "the repairing scan");
        assert_eq!(
            entry(&failing_id),
            Some(("claude/example/failing".to_owned(), Kind::Record))
        );
        assert_eq!(
            lookup(&failing_id),
            (vec!["claude/example/failing".to_owned()], false)
        );
        TRIPPED.store(0, Ordering::SeqCst);

        // #1400 review: an index this process may not write is never retried.
        // With ids/ read-only, every lookup scans, yet none asks for the
        // writer lock or tries to write, and a writer skips its note.
        use std::os::unix::fs::PermissionsExt;
        let stuck = "claude/example/stuck";
        let (mut stuck_record, stuck_id) = saved(stuck, "run-stuck");
        std::fs::remove_file(path(&stuck_id)).unwrap();
        let mode = |bits| {
            std::fs::set_permissions(directory(), std::fs::Permissions::from_mode(bits)).unwrap()
        };
        let counts = || (LOCKS.load(Ordering::SeqCst), CHANGES.load(Ordering::SeqCst));
        mode(0o500);
        let before = counts();
        for _ in 0..20 {
            assert_eq!(lookup(&stuck_id), (vec![stuck.to_owned()], true));
        }
        storage::write(&mut stuck_record).unwrap();
        assert_eq!(
            counts(),
            before,
            "no lock and no write while ids/ is read-only"
        );
        mode(0o700);
        // zerus-2sd29: an ids/ owned by another user is refused by its
        // metadata too, before any lock or write, also after the interval.
        TRIPPED.store(0, Ordering::SeqCst);
        FOREIGN.store(true, Ordering::SeqCst);
        let before = counts();
        for _ in 0..20 {
            assert_eq!(lookup(&stuck_id), (vec![stuck.to_owned()], true));
        }
        storage::write(&mut stuck_record).unwrap();
        LATER_MS.fetch_add(RETRY_MS, Ordering::SeqCst);
        assert_eq!(lookup(&stuck_id), (vec![stuck.to_owned()], true));
        assert_eq!(
            counts(),
            before,
            "no lock and no write while ids/ belongs to another user"
        );
        FOREIGN.store(false, Ordering::SeqCst);
        // A directory at an entry is never written over and reads as a miss.
        TRIPPED.store(0, Ordering::SeqCst);
        std::fs::create_dir(path(&stuck_id)).unwrap();
        let before = counts();
        storage::write(&mut stuck_record).unwrap();
        for _ in 0..3 {
            assert_eq!(lookup(&stuck_id), (vec![stuck.to_owned()], true));
        }
        assert_eq!(counts(), before, "no lock and no write over a directory");
        std::fs::remove_dir(path(&stuck_id)).unwrap();
        // A FIFO at an entry is a miss, never a hang, for a lookup and a write.
        TRIPPED.store(0, Ordering::SeqCst);
        let fifo = std::ffi::CString::new(path(&stuck_id).to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let (sender, receiver) = std::sync::mpsc::channel();
        let id = stuck_id.clone();
        std::thread::spawn(move || {
            let mut record = storage::read(stuck).unwrap();
            storage::write(&mut record).unwrap();
            let _ = sender.send(session_id::names_of(&id).unwrap());
        });
        let names = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("a FIFO entry never blocks");
        assert_eq!(names, vec![stuck.to_owned()]);
        std::fs::remove_file(path(&stuck_id)).unwrap();
        TRIPPED.store(0, Ordering::SeqCst);

        // The bound: with 50,000 other records, a hit reads one entry and one
        // record and never scans.
        for n in 0..50_000 {
            let name = format!("claude/filler/{n}");
            let path = root().join(format!("{:x}.json", Sha256::digest(name.as_bytes())));
            std::fs::write(path, format!("{}\n", claude(&name, &format!("filler-{n}")))).unwrap();
        }
        let (_, bound_id) = saved("claude/example/bound", "run-bound");
        let started = std::time::Instant::now();
        assert_eq!(
            lookup(&bound_id),
            (vec!["claude/example/bound".to_owned()], false)
        );
        std::env::set_var("HGS_SESSION_ID", &bound_id);
        let before = scans();
        assert!(storage::read_own_run("claude/example/bound", "run-bound")
            .unwrap()
            .is_some());
        assert_eq!(scans(), before);
        std::env::remove_var("HGS_SESSION_ID");
        eprintln!(
            "two indexed lookups among 50,000 records took {:?}",
            started.elapsed()
        );
    }
}
