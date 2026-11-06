//! Real read-only DB restrictions, with synthetic native origins. These do
//! not qualify original-child authentication or any stop/Force permission.
use super::*;
use crate::desktop::ownership::programs::Current;
use zerus_client_core::worktree_programs::stop::Source;

fn until() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

#[test]
fn current_service_requires_the_original_live_owner_grant_image_and_configuration() {
    for change in 0..7 {
        let mut f = Fixture::new();
        let grant = f.request(Action::Grant);
        assert!(f.apply(grant).service_granted());
        let mut owner = Some(f.location.lock().unwrap());
        let source = Source::Service {
            epoch: f.epoch.clone(),
        };
        let original = Current::capture(&f.location, &source, f.origin.clone(), until()).unwrap();
        original.check(until()).unwrap();
        match change {
            0 => {
                f.store
                    .db
                    .execute(
                        "UPDATE service SET epoch=?1",
                        [uuid::Uuid::new_v4().to_string()],
                    )
                    .unwrap();
            }
            1 => {
                f.store
                    .db
                    .execute("UPDATE service SET configuration=?1", ["b".repeat(64)])
                    .unwrap();
            }
            2 => {
                f.store
                    .db
                    .execute(
                        "UPDATE desktop_ownership SET owner_epoch=?1",
                        [uuid::Uuid::new_v4().to_string()],
                    )
                    .unwrap();
            }
            3 => {
                f.store
                    .db
                    .execute_batch("UPDATE desktop_ownership SET rearm_required=1")
                    .unwrap();
            }
            4 => {
                f.store
                    .db
                    .execute_batch("UPDATE desktop_ownership SET state='native'")
                    .unwrap();
            }
            5 => {
                std::fs::write(&f.origin.image, b"changed original native frontend").unwrap();
            }
            _ => {
                drop(owner.take());
            }
        }
        assert!(original.check(until()).is_err(), "change {change}");
    }
}

#[test]
fn native_requires_the_original_takeover_without_borrowing_a_service_grant() {
    for change in 0..5 {
        let mut f = Fixture::new();
        let request = f.request(Action::NativeTakeover);
        let receipt = f.apply(request);
        assert!(receipt.native_ready());
        let source = Source::Native {
            request_id: receipt.request_id,
            journal: receipt.journal,
            revision: receipt.revision,
        };
        let original = Current::capture(&f.location, &source, f.origin.clone(), until()).unwrap();
        original.check(until()).unwrap();
        match change {
            0 => {
                f.store
                    .db
                    .execute(
                        "UPDATE desktop_ownership SET request_id=?1",
                        [uuid::Uuid::new_v4().to_string()],
                    )
                    .unwrap();
            }
            1 => {
                f.store
                    .db
                    .execute_batch("UPDATE desktop_ownership SET revision=revision+1")
                    .unwrap();
            }
            2 => {
                f.store
                    .db
                    .execute(
                        "UPDATE desktop_ownership SET journal=?1",
                        [uuid::Uuid::new_v4().to_string()],
                    )
                    .unwrap();
            }
            3 => {
                f.store
                    .db
                    .execute("UPDATE service SET configuration=?1", ["b".repeat(64)])
                    .unwrap();
            }
            _ => {
                f.store
                    .db
                    .execute_batch("UPDATE desktop_ownership SET state='service'")
                    .unwrap();
            }
        }
        assert!(original.check(until()).is_err(), "change {change}");
    }
}

#[test]
fn a_missing_native_journal_stays_absent_and_cannot_become_a_service_or_a_new_database() {
    let f = Fixture::new();
    let location = Location {
        root: f.base.join("empty/desktop"),
        configuration: f.location.configuration.clone(),
    };
    location.prepare().unwrap();
    let source = Source::Native {
        request_id: uuid::Uuid::new_v4().to_string(),
        journal: None,
        revision: 0,
    };
    let original = Current::capture(&location, &source, f.origin.clone(), until()).unwrap();
    assert!(!location.root.join("state.sqlite3").exists());
    original.check(until()).unwrap();
    assert!(Current::capture(
        &location,
        &Source::Service {
            epoch: uuid::Uuid::new_v4().to_string()
        },
        f.origin.clone(),
        until()
    )
    .is_err());
    assert!(!location.root.join("state.sqlite3").exists());
    let _new = Store::open(&location, &uuid::Uuid::new_v4().to_string()).unwrap();
    assert!(
        original.check(until()).is_err(),
        "new state cannot replace the original absent scope"
    );
    assert!(original.check(Instant::now()).is_err());
}

#[test]
fn identical_database_bytes_at_a_new_object_cannot_replace_the_retained_source() {
    let mut f = Fixture::new();
    let request = f.request(Action::NativeTakeover);
    let receipt = f.apply(request);
    let source = Source::Native {
        request_id: receipt.request_id,
        journal: receipt.journal,
        revision: receipt.revision,
    };
    let original = Current::capture(&f.location, &source, f.origin.clone(), until()).unwrap();
    f.store
        .db
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(f.store);
    let path = f.location.root.join("state.sqlite3");
    let replacement = f.location.root.join("private-copy.sqlite3");
    std::fs::copy(&path, &replacement).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        std::fs::read(&replacement).unwrap()
    );
    std::fs::rename(&path, f.location.root.join("retained-old.sqlite3")).unwrap();
    std::fs::rename(replacement, &path).unwrap();
    assert!(
        original.check(until()).is_err(),
        "equal rows and scope are not the original database object"
    );
}
