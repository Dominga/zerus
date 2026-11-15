//! The native leaf is created only for a wholly absent legacy leaf. Existing
//! identities and receipts are checked as-is, never reconstructed or repaired.
use super::*;

const OBJECTS: &[(&str, &str, &str)] = &[
    (
        "person_gesture_store",
        "table",
        "CREATE TABLE person_gesture_store(
         id INTEGER PRIMARY KEY CHECK(id=1),
         nonce TEXT NOT NULL CHECK(length(nonce)=36))",
    ),
    (
        "person_gesture_store_immutable",
        "trigger",
        "CREATE TRIGGER person_gesture_store_immutable BEFORE UPDATE ON person_gesture_store
         BEGIN SELECT RAISE(ABORT,'Person gesture store identity is immutable'); END",
    ),
    (
        "person_gesture_store_kept",
        "trigger",
        "CREATE TRIGGER person_gesture_store_kept BEFORE DELETE ON person_gesture_store
         BEGIN SELECT RAISE(ABORT,'Person gesture store identity is kept'); END",
    ),
    (
        "person_gestures",
        "table",
        "CREATE TABLE person_gestures(
         request_id TEXT PRIMARY KEY REFERENCES requests(id),
         intent TEXT NOT NULL CHECK(length(CAST(intent AS BLOB))<=65536))",
    ),
    (
        "person_gestures_immutable",
        "trigger",
        "CREATE TRIGGER person_gestures_immutable BEFORE UPDATE ON person_gestures
         BEGIN SELECT RAISE(ABORT,'Person gesture evidence is immutable'); END",
    ),
];

fn incomplete() -> Problem {
    Problem::new(
        code::INVALID_STATE,
        "Native Person evidence is incomplete or corrupt; no team data was changed.",
    )
}

fn definitions(db: &Connection) -> Result<usize> {
    let mut present = 0;
    for (name, kind, sql) in OBJECTS {
        let definition: Option<(String, Option<String>)> = db
            .query_row(
                "SELECT type,CASE WHEN length(CAST(sql AS BLOB))<=2048 THEN sql END FROM sqlite_master WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((actual_kind, actual_sql)) = definition {
            let normalize = |s: &str| s.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
            if actual_kind != *kind
                || actual_sql.as_deref().map(normalize).as_deref() != Some(normalize(sql).as_str())
            {
                return Err(incomplete());
            }
            present += 1;
        }
    }
    Ok(present)
}

pub(super) fn validate(db: &Connection) -> Result<()> {
    if definitions(db)? != OBJECTS.len() {
        return Err(incomplete());
    }
    let nonce: Option<String> = db
        .query_row(
            "SELECT nonce FROM person_gesture_store WHERE id=1 AND length(CAST(nonce AS BLOB))=36",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if !nonce
        .as_deref()
        .is_some_and(zerus_desktop_migration::canonical_uuid)
    {
        return Err(incomplete());
    }
    Ok(())
}

/// Caller owns the immediate transaction covering extensions and core mark.
pub(super) fn migrate(db: &Connection) -> Result<()> {
    if definitions(db)? == 0 {
        for (_, _, sql) in OBJECTS {
            db.execute_batch(sql).map_err(storage)?;
        }
        db.execute(
            "INSERT INTO person_gesture_store VALUES(1,?1)",
            [uuid::Uuid::new_v4().to_string()],
        )
        .map_err(storage)?;
    }
    validate(db)
}
