use super::*;
use crate::desktop::{revoke::test_support, store::Store};
use std::{collections::BTreeMap, time::SystemTime};
use zerus_desktop_migration::HANDOFF_FILE;

struct Fixture {
    _temp: tempfile::TempDir,
    location: Location,
    store: Option<Store>,
    epoch: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap().join("state/desktop");
        let location = Location {
            root,
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        let store = Store::open(&location, &epoch).unwrap();
        Self {
            _temp: temp,
            location,
            store: Some(store),
            epoch,
        }
    }
    fn settle(&mut self, id: &str) -> Value {
        test_support::settle(&self.location, self.store.as_mut().unwrap(), id, None).unwrap()
    }
    fn sql(&self, sql: &str) {
        self.store.as_ref().unwrap().db.execute_batch(sql).unwrap();
    }
    /// The service is stopped: its own connection is closed, as offline.
    fn stop(&mut self) {
        self.store.take();
    }
}

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

type Snapshot = BTreeMap<&'static str, Option<(String, usize, SystemTime)>>;

/// The bytes (as their SHA-256), length and modification time of every desktop
/// state leaf.
fn snapshot(location: &Location) -> Snapshot {
    use sha2::{Digest, Sha256};
    LEAVES
        .iter()
        .map(|leaf| {
            let path = location.root.join(leaf);
            let value = std::fs::read(&path).ok().map(|bytes| {
                (
                    format!("{:x}", Sha256::digest(&bytes)),
                    bytes.len(),
                    std::fs::metadata(&path).unwrap().modified().unwrap(),
                )
            });
            (*leaf, value)
        })
        .collect()
}

/// The main database, journal and marker keep their bytes and mtime. The real
/// read-only lookup of a cleanly stopped database may add only SQLite's reader
/// sidecars where they were absent (an empty -wal and the -shm index); an
/// existing -shm keeps its bytes, although the reader may touch its mtime.
fn assert_kept(before: &Snapshot, after: &Snapshot, label: &str) {
    for leaf in LEAVES {
        match (leaf, &before[leaf], &after[leaf]) {
            ("state.sqlite3-wal", None, Some((_, 0, _))) | ("state.sqlite3-shm", None, Some(_)) => {
            }
            ("state.sqlite3-shm", Some(kept), Some(now)) => {
                assert_eq!(
                    (&now.0, now.1),
                    (&kept.0, kept.1),
                    "{label}: -shm bytes changed"
                )
            }
            (_, kept, now) => assert_eq!(now, kept, "{label}: {leaf} changed"),
        }
    }
}

/// A refusal changes no state byte or mtime and releases both exclusions.
fn refused(location: &Location, id: &str, reason: &str) {
    let before = snapshot(location);
    let Err(error) = Offline::acquire(location, id) else {
        panic!("{reason}: removal must refuse");
    };
    assert!(format!("{error:#}").contains(reason), "{reason}: {error:#}");
    assert_kept(&before, &snapshot(location), reason);
    assert!(location.lock().is_ok(), "{reason}: owner.lock kept");
    assert!(
        location.handoff_lock().is_ok(),
        "{reason}: handoff lock kept"
    );
}

/// An admitted removal holds owner.lock and the handoff lock until dropped.
fn admitted(location: &Location, id: &str) -> Offline {
    let offline = Offline::acquire(location, id).unwrap();
    offline.admit(location, id).unwrap();
    assert!(location.lock().is_err(), "owner.lock is held");
    assert!(location.handoff_lock().is_err(), "handoff lock is held");
    offline
}

#[test]
fn no_state_needs_no_proof_but_state_appearing_before_the_effect_refuses() {
    let temp = tempfile::tempdir().unwrap();
    let location = Location {
        root: temp.path().canonicalize().unwrap().join("state/desktop"),
        configuration: "a".repeat(64),
    };
    let request = id();
    let offline = admitted(&location, &request);
    assert!(offline.receipt().is_none());
    std::fs::write(location.root.join("state.sqlite3-wal"), b"").unwrap();
    assert!(offline.admit(&location, &request).is_err());
    drop(offline);
    assert!(location.lock().is_ok() && location.handoff_lock().is_ok());
}

#[test]
fn a_sidecar_or_marker_without_its_database_refuses() {
    let temp = tempfile::tempdir().unwrap();
    let location = Location {
        root: temp.path().canonicalize().unwrap().join("state/desktop"),
        configuration: "a".repeat(64),
    };
    location.prepare().unwrap();
    for leaf in [
        "state.sqlite3-wal",
        "state.sqlite3-shm",
        "state.sqlite3-journal",
        HANDOFF_FILE,
    ] {
        let path = location.root.join(leaf);
        std::fs::write(&path, b"").unwrap();
        refused(&location, &id(), "desktop state is incomplete");
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn a_passive_null_writer_history_is_admitted_with_its_exact_receipt() {
    let mut f = Fixture::new();
    let request = id();
    let settled = f.settle(&request);
    assert!(settled["writer_epoch"].is_null());
    f.stop();
    let offline = admitted(&f.location, &request);
    let receipt = offline.receipt().unwrap();
    for field in ["accepted", "durable", "revoked", "checkpointed"] {
        assert_eq!(receipt[field], true, "{field}");
    }
    assert_eq!(receipt["request_id"], request);
    assert_eq!(receipt["configuration"], f.location.configuration);
    assert!(receipt["writer_epoch"].is_null());
}

#[test]
fn a_background_history_with_its_revoked_marker_is_admitted() {
    let mut f = Fixture::new();
    let writer = id();
    test_support::marker(&f.location, &writer, "complete");
    let request = id();
    assert_eq!(f.settle(&request)["writer_epoch"], writer);
    f.stop();
    let offline = admitted(&f.location, &request);
    assert_eq!(offline.receipt().unwrap()["writer_epoch"], writer);
}

#[test]
fn a_later_grant_and_newer_rollback_supersede_the_older_proof_even_with_a_revoked_marker() {
    let mut f = Fixture::new();
    let first_writer = id();
    test_support::marker(&f.location, &first_writer, "complete");
    let first = id();
    f.settle(&first);
    // A later grant and new handoff, then a second settled rollback.
    f.sql("UPDATE desktop_ownership SET state='service',rearm_required=0 WHERE id=1; UPDATE desktop_writer SET revoked=0 WHERE id=1;");
    let second_writer = id();
    test_support::marker(&f.location, &second_writer, "complete");
    let second = id();
    assert_eq!(f.settle(&second)["writer_epoch"], second_writer);
    f.stop();
    refused(&f.location, &first, "writer_authority_changed");
    drop(admitted(&f.location, &second));
    // A Revoked marker naming the first writer lends nothing to either.
    std::fs::write(
        f.location.root.join(HANDOFF_FILE),
        std::fs::read(f.location.root.join(HANDOFF_FILE))
            .map(|bytes| {
                String::from_utf8(bytes)
                    .unwrap()
                    .replace(&second_writer, &first_writer)
            })
            .unwrap(),
    )
    .unwrap();
    refused(&f.location, &first, "writer_authority_changed");
    refused(&f.location, &second, "writer_epoch_changed");
}

#[test]
fn a_current_grant_or_writer_without_a_marker_refuses() {
    let mut f = Fixture::new();
    let request = id();
    f.settle(&request);
    f.sql("UPDATE desktop_ownership SET state='service',rearm_required=0 WHERE id=1;");
    f.stop();
    refused(&f.location, &request, "writer_authority_changed");
    let mut f = Fixture::new();
    let request = id();
    f.settle(&request);
    f.sql("UPDATE desktop_writer SET revoked=0 WHERE id=1;");
    f.stop();
    refused(&f.location, &request, "writer_epoch_changed");
}

#[test]
fn a_marker_naming_another_writer_refuses() {
    let mut f = Fixture::new();
    let writer = id();
    test_support::marker(&f.location, &writer, "complete");
    let request = id();
    f.settle(&request);
    f.stop();
    let path = f.location.root.join(HANDOFF_FILE);
    let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    std::fs::write(&path, text.replace(&writer, &id())).unwrap();
    refused(&f.location, &request, "writer_epoch_changed");
}

#[test]
fn missing_prepared_malformed_conflicting_or_invalid_receipts_refuse() {
    let mut f = Fixture::new();
    let pending = id();
    test_support::prepare_only(&f.location, f.store.as_mut().unwrap(), &pending);
    let malformed = id();
    let conflicting = id();
    // Settling another request would make it the newest command; check the
    // prepared one first, against its own store.
    f.stop();
    refused(&f.location, &pending, "revocation_pending");
    refused(&f.location, &id(), "request_not_found");
    refused(&f.location, "not-a-request-id", "invalid_request_id");
    let mut f = Fixture::new();
    f.settle(&malformed);
    f.sql(&format!(
        "UPDATE desktop_commands SET result='{{}}' WHERE request_id='{malformed}'"
    ));
    f.stop();
    refused(&f.location, &malformed, "missing field");
    let mut f = Fixture::new();
    f.settle(&conflicting);
    f.sql(&format!(
        "UPDATE desktop_commands SET fingerprint='{}' WHERE request_id='{conflicting}'",
        "0".repeat(64)
    ));
    f.stop();
    refused(&f.location, &conflicting, "request_conflict");
}

#[test]
fn undrained_operation_resources_refuse() {
    let mut f = Fixture::new();
    let request = id();
    f.settle(&request);
    let original = zerus_desktop_ledger::operations::cache::Original {
        preparation_id: id(),
        operation_id: id(),
        configuration: f.location.configuration.clone(),
        root: "/srv/example/cache".into(),
        limit: 1,
        defer_ms: 0,
        policy_change: true,
        revision: 0,
        service_epoch: f.epoch.clone(),
        writer_epoch: id(),
        lease: id(),
    };
    let store = f.store.as_mut().unwrap();
    let tx = store.db.transaction().unwrap();
    zerus_desktop_ledger::operations::cache::begin(&tx, &original, &"f".repeat(64)).unwrap();
    tx.commit().unwrap();
    f.stop();
    refused(&f.location, &request, "operation_resources_not_drained");
}

#[test]
fn owner_or_retained_handoff_contention_refuses_without_changes() {
    let mut f = Fixture::new();
    let request = id();
    f.settle(&request);
    f.stop();
    let before = snapshot(&f.location);
    let owner = f.location.lock().unwrap();
    assert!(Offline::acquire(&f.location, &request).is_err());
    drop(owner);
    // A Windows Runtime or delegate retains the serve owner's handoff loan.
    let delegate = f.location.handoff_lock().unwrap();
    assert!(Offline::acquire(&f.location, &request).is_err());
    assert!(f.location.lock().is_ok(), "the refusal released owner.lock");
    drop(delegate);
    assert_eq!(snapshot(&f.location), before, "contention reads no state");
    drop(admitted(&f.location, &request));
}

#[test]
fn an_unsafe_or_replaced_database_or_marker_refuses() {
    let mut f = Fixture::new();
    let writer = id();
    test_support::marker(&f.location, &writer, "complete");
    let request = id();
    f.settle(&request);
    f.stop();
    let database = f.location.root.join("state.sqlite3");
    let alias = f.location.root.join("database-alias");
    std::fs::hard_link(&database, &alias).unwrap();
    refused(&f.location, &request, "single-link");
    std::fs::remove_file(&alias).unwrap();
    // A copy renamed over the checked database after the proof: refused at
    // admission on Unix; on Windows the retained Verification (no delete
    // sharing) prevents the replacement itself.
    let offline = Offline::acquire(&f.location, &request).unwrap();
    let copy = f.location.root.join("database-copy");
    std::fs::copy(&database, &copy).unwrap();
    #[cfg(unix)]
    {
        std::fs::rename(&copy, &database).unwrap();
        assert!(
            format!("{:#}", offline.admit(&f.location, &request).unwrap_err())
                .contains("desktop state changed")
        );
    }
    #[cfg(windows)]
    {
        assert!(std::fs::rename(&copy, &database).is_err(), "held: refused");
        offline.admit(&f.location, &request).unwrap();
        drop(offline);
        // The protection lasts exactly as long as the guard.
        std::fs::rename(&copy, &database).unwrap();
    }
    #[cfg(unix)]
    drop(offline);
    // A marker changed after the proof.
    let offline = Offline::acquire(&f.location, &request).unwrap();
    let path = f.location.root.join(HANDOFF_FILE);
    let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    std::fs::write(&path, text.replace(&writer, &id())).unwrap();
    assert!(offline.admit(&f.location, &request).is_err());
}

/// Pinned behaviour of the real read-only lookup of a cleanly stopped
/// service's WAL database: the main database and marker keep their bytes and
/// mtime; SQLite's read-only WAL reader creates an empty -wal and the -shm
/// index, and a read-only connection leaves them in place on close. Nothing is
/// checkpointed, migrated or deleted.
#[test]
fn the_read_only_lookup_keeps_the_database_and_adds_only_reader_sidecars() {
    let mut f = Fixture::new();
    let writer = id();
    test_support::marker(&f.location, &writer, "complete");
    let request = id();
    f.settle(&request);
    f.stop();
    let before = snapshot(&f.location);
    assert!(before["state.sqlite3-wal"].is_none() && before["state.sqlite3-shm"].is_none());
    let offline = admitted(&f.location, &request);
    let held = snapshot(&f.location);
    assert_kept(&before, &held, "while held");
    drop(offline);
    let after = snapshot(&f.location);
    assert_kept(&before, &after, "after release");
    assert_eq!(after["state.sqlite3-wal"].as_ref().map(|v| v.1), Some(0));
    assert!(after["state.sqlite3-shm"].is_some());
    assert_eq!(after["state.sqlite3"], before["state.sqlite3"]);
    assert_eq!(after[HANDOFF_FILE], before[HANDOFF_FILE]);
    // A second removal attempt over those sidecars keeps them as they are.
    drop(admitted(&f.location, &request));
    assert_eq!(
        snapshot(&f.location)["state.sqlite3"],
        before["state.sqlite3"]
    );
}

/// desktop-u's G1 control on #1453: a held lock file renamed aside and a new
/// holder of the lock at the original pathname. Unix cannot prevent it, so
/// admission and confirmation refuse; on Windows the pin prevents both the
/// rename and the deletion, and the held exclusion stays admitted.
fn replaced_exclusion(leaf: &str) {
    let mut f = Fixture::new();
    let request = id();
    f.settle(&request);
    f.stop();
    let offline = admitted(&f.location, &request);
    let path = f.location.root.join(leaf);
    let kept = f.location.root.join(format!("{leaf}.kept"));
    #[cfg(unix)]
    {
        std::fs::rename(&path, &kept).unwrap();
        let _other: Box<dyn std::any::Any> = if leaf == "owner.lock" {
            Box::new(f.location.lock().unwrap())
        } else {
            Box::new(f.location.handoff_lock().unwrap())
        };
        assert!(
            format!("{:#}", offline.admit(&f.location, &request).unwrap_err())
                .contains("desktop exclusion changed"),
            "{leaf}"
        );
        assert!(offline.confirm(&f.location).is_err(), "{leaf}");
    }
    #[cfg(windows)]
    {
        assert!(std::fs::rename(&path, &kept).is_err(), "{leaf} is pinned");
        assert!(std::fs::remove_file(&path).is_err(), "{leaf} is pinned");
        offline.admit(&f.location, &request).unwrap();
        offline.confirm(&f.location).unwrap();
        drop(offline);
        // The pin lasts exactly as long as the guard.
        std::fs::rename(&path, &kept).unwrap();
        std::fs::rename(&kept, &path).unwrap();
    }
}

#[test]
fn a_replaced_owner_lock_never_keeps_the_offline_proof() {
    replaced_exclusion("owner.lock");
}

#[test]
fn a_replaced_handoff_lock_never_keeps_the_offline_proof() {
    replaced_exclusion(zerus_desktop_migration::HANDOFF_LOCK);
}

/// A held lock whose pathname disappears, or a replaced location folder,
/// refuses on Unix; on Windows the pinned folder cannot be renamed.
#[test]
fn a_missing_lock_or_replaced_location_never_keeps_the_offline_proof() {
    let mut f = Fixture::new();
    let request = id();
    f.settle(&request);
    f.stop();
    let offline = admitted(&f.location, &request);
    let aside = f.location.root.with_file_name("desktop-aside");
    #[cfg(unix)]
    {
        std::fs::remove_file(f.location.root.join("owner.lock")).unwrap();
        assert!(offline.admit(&f.location, &request).is_err());
        assert!(offline.confirm(&f.location).is_err());
        drop(offline);
        let offline = admitted(&f.location, &request);
        std::fs::rename(&f.location.root, &aside).unwrap();
        f.location.prepare().unwrap();
        assert!(offline.admit(&f.location, &request).is_err());
        assert!(offline.confirm(&f.location).is_err());
    }
    #[cfg(windows)]
    {
        assert!(std::fs::rename(&f.location.root, &aside).is_err());
        offline.admit(&f.location, &request).unwrap();
        offline.confirm(&f.location).unwrap();
        drop(offline);
        std::fs::rename(&f.location.root, &aside).unwrap();
        std::fs::rename(&aside, &f.location.root).unwrap();
    }
}

/// A stable held exclusion stays admitted and confirmed, and excludes a
/// second removal until it is released.
#[test]
fn a_held_exclusion_stays_admitted_and_excludes_a_second_removal() {
    let mut f = Fixture::new();
    let request = id();
    f.settle(&request);
    f.stop();
    let offline = admitted(&f.location, &request);
    offline.confirm(&f.location).unwrap();
    assert!(Offline::acquire(&f.location, &request).is_err());
    offline.admit(&f.location, &request).unwrap();
    offline.confirm(&f.location).unwrap();
    drop(offline);
    let again = admitted(&f.location, &request);
    again.confirm(&f.location).unwrap();
}
