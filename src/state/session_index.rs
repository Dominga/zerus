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
//! only costs the next lookup a scan.
use super::*;
use std::fs;
use std::io::Read;

/// Entries are tiny; anything larger is not one of ours.
const ENTRY_BYTES: u64 = 4096;

/// Where a session's record lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// A session record (`storage::read`).
    Record,
    /// A DeepSeek binding (`dsh::binding`).
    DeepSeek,
    /// An OpenCode binding (`opencode::binding`).
    OpenCode,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Record => "record",
            Kind::DeepSeek => "dsh",
            Kind::OpenCode => "opencode",
        }
    }
    fn parse(text: &str) -> Option<Kind> {
        match text {
            "record" => Some(Kind::Record),
            "dsh" => Some(Kind::DeepSeek),
            "opencode" => Some(Kind::OpenCode),
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

/// The entry for `id`, when there is a well-formed one.
fn entry(id: &str) -> Option<(String, Kind)> {
    if !session_id::canonical(id) {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path(id))
        .ok()?
        .take(ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
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
    let value = json!({"version": 1, "name": name, "kind": kind.as_str()});
    storage::atomic(&path(id), &format!("{value}\n"))
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
    if entry(id).is_some_and(|(noted, noted_kind)| noted == name && noted_kind == kind) {
        return;
    }
    let _ = write_entry(id, name, kind);
}

/// Whether `record`, read as `name`, is the live or saved session `id`.
fn carries(record: &Value, id: &str, name: &str) -> bool {
    string(record, "session_id") == id
        && string(record, "name") == name
        && string(record, "archive_id").is_empty()
        && record["rename_shadow"] != true
}

/// The live or saved session `id`, when its entry still verifies: the record
/// of that kind under that label carries the ID, with no archive and no rename
/// shadow. None is a miss; the caller scans.
pub(super) fn find(id: &str) -> Option<Found> {
    let (name, kind) = entry(id)?;
    let record = match kind {
        Kind::Record => storage::record_path(&name)
            .exists()
            .then(|| storage::read(&name).ok())
            .flatten()?,
        Kind::DeepSeek => dsh::binding(&name).ok()?,
        Kind::OpenCode => opencode::binding(&name).ok()?,
    };
    carries(&record, id, &name).then_some(Found { name, kind, record })
}

/// After a scan for `id` found `found` (label and kind, at most one label for
/// a sound state): points the entry at it, or removes a stale entry when the
/// scan found none. Under the state writer lock, only when it is free right
/// now: a held lock, which may be this command's own, skips the repair.
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
    if current == wanted || (wanted.is_none() && !path(id).exists()) {
        return;
    }
    let Ok(Some(_guard)) = storage::writer_until(std::time::Instant::now()) else {
        return;
    };
    let _ = match wanted {
        Some((name, kind)) => write_entry(id, &name, kind),
        None => fs::remove_file(path(id))
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(|error| error.to_string()),
    };
}

/// How many full scans the lookups made, for the bound tests.
#[cfg(test)]
pub(super) static SCANS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Counts one full scan (tests only).
pub(super) fn scanned() {
    #[cfg(test)]
    SCANS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
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

        // Archive and forget: the entry goes stale, one scan finds nothing and
        // removes it.
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
            assert_eq!(
                lookup(&reminted),
                (vec!["claude/example/older".to_owned()], true)
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
        assert_eq!(
            own("run-hooked"),
            (Some("claude/example/hooked-2".to_owned()), true)
        );
        assert_eq!(
            own("run-hooked"),
            (Some("claude/example/hooked-2".to_owned()), false),
            "repaired by the hook's scan"
        );
        std::env::remove_var("HGS_SESSION_ID");

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
