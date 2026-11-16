//! Canonical Team/request rows come from the operation writer. Only the
//! schema is reduced to represent the native-4 release before publication.
use super::*;
use crate::teams::ops::tests::{self as producers, Harness, TEAM};

pub(crate) fn native_four(h: &Harness) {
    let db = h.store.db.as_ref().unwrap();
    for table in [
        "person_action_originals",
        "phase_publications",
        "phase_reworks",
    ] {
        let present: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        if present {
            let count: u32 = db
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                count, 0,
                "legacy schema fixture cannot erase {table} evidence"
            );
        }
    }
    let tx = db.unchecked_transaction().unwrap();
    for (name, kind, _) in OBJECTS.iter().rev() {
        tx.execute_batch(&format!("DROP {kind} IF EXISTS {name}"))
            .unwrap();
    }
    tx.execute_batch(
        "DROP TABLE IF EXISTS phase_reworks;
         DROP INDEX IF EXISTS phase_publication_episodes;
         DROP TABLE IF EXISTS phase_cards;
         DROP TABLE IF EXISTS phase_draft_previous;
         DROP TABLE IF EXISTS phase_draft_dependencies;
         DROP TABLE IF EXISTS phase_publication_nodes;
         DROP TABLE IF EXISTS phase_publications;
         DROP TABLE IF EXISTS phase_reviews;",
    )
    .unwrap();
    tx.pragma_update(None, "user_version", versions::CORE_NATIVE_PERSON)
        .unwrap();
    tx.commit().unwrap();
}

fn kept(db: &Connection) -> (String, Vec<String>, Vec<String>) {
    let nonce = db
        .query_row(
            "SELECT nonce FROM person_gesture_store WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut statement = db
        .prepare("SELECT json_array(id,team,actor,content,result) FROM requests ORDER BY id")
        .unwrap();
    let rows = statement
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let mut statement = db
        .prepare("SELECT json_array(request_id,intent) FROM person_gestures ORDER BY request_id")
        .unwrap();
    let gestures = statement
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    (nonce, rows, gestures)
}

fn native_receipt(h: &mut Harness) {
    let card = h
        .run(
            &producers::service(),
            ops::Operation::OpenCard {
                kind: "launch_refused".into(),
                text: "Private migration control".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .unwrap()
        .item
        .unwrap();
    h.run(
        &producers::person(),
        ops::Operation::DecideCard {
            card,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    assert_eq!(kept(h.store.db.as_ref().unwrap()).2.len(), 1);
}

#[test]
fn native_seven_migrates_native_four_without_inventing_rework_or_changing_originals() {
    let mut h = Harness::new();
    native_receipt(&mut h);
    native_four(&h);
    let before = kept(h.store.db.as_ref().unwrap());
    let team = h.store.team(TEAM).unwrap().unwrap();
    let store = Store::open(h.temp.path()).unwrap();
    let db = store.db.as_ref().unwrap();
    let version: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, versions::CORE_NATIVE_FLOOR);
    validate(db).unwrap();
    rework::validate(db).unwrap();
    assert_eq!(kept(db), before);
    assert_eq!(store.team(TEAM).unwrap().unwrap(), team);
    for table in [
        "phase_reworks",
        "phase_publications",
        "phase_publication_nodes",
        "person_action_originals",
    ] {
        let count: u32 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "migration cannot invent a {table} fact");
    }
}

#[test]
fn native_seven_rejects_partial_current_evidence_before_open_or_original_replay() {
    for (kind, object) in [
        ("TRIGGER", "person_action_original_immutable"),
        ("TABLE", "phase_reworks"),
        ("TABLE", "phase_publication_nodes"),
        ("TRIGGER", "person_gestures_immutable"),
    ] {
        let mut h = Harness::new();
        native_receipt(&mut h);
        let who = producers::service();
        let request = producers::request(
            h.store.team(TEAM).unwrap().unwrap().revision,
            ops::Operation::OpenCard {
                kind: "launch_refused".into(),
                text: "Saved service original".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        );
        ops::apply(&mut h.store, &who, &request, 1234).unwrap();
        let before = kept(h.store.db.as_ref().unwrap());
        h.store
            .db
            .as_ref()
            .unwrap()
            .execute_batch(&format!("DROP {kind} {object}"))
            .unwrap();
        assert!(Store::open(h.temp.path()).is_err(), "missing {object}");
        assert!(
            ops::apply(&mut h.store, &who, &request, 1234).is_err(),
            "cached writer cannot replay with missing {object}"
        );
        let db = h.store.db.as_ref().unwrap();
        assert_eq!(kept(db), before);
        let present: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
                [object],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!present);
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            versions::CORE_NATIVE_FLOOR
        );
    }
}
