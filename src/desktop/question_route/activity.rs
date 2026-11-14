//! The read-only Activity lines of submitted question routes
//! (zerus-sa9qn.33): for one session, the newest routes the actor submitted
//! where it is the child or the parent, each with the child's name and
//! reference, the parent's name and the route's summary. The child's text
//! appears only as the route's own preview of at most
//! [`zerus_client_core::desktop::question_route::PREVIEW_CHARS`] characters;
//! the parent's line also carries the exact text it received, so its own
//! "You" message can fold into the line on an exact match.
//!
//! It reads the route ledger by its schema-38 indexes and each route's own
//! saved original. It never sends, settles, retries or grants anything, and
//! reserved routes remain pending. Settled failures are shown only to the child.
use rusqlite::{params, Connection, Transaction};
use serde_json::{json, Value};

pub(super) const OP: &str = "question_route_activity";
/// The newest submitted routes one request returns.
pub(super) const LIMIT: usize = 20;

pub(super) fn migrate(tx: &Transaction<'_>) -> anyhow::Result<()> {
    tx.execute_batch(
        "CREATE INDEX desktop_question_routes_child_submitted
        ON desktop_question_routes(child_session_id,created,request_id) WHERE state='submitted';
        CREATE INDEX desktop_question_routes_parent_submitted
        ON desktop_question_routes(parent_session_id,created,request_id) WHERE state='submitted';",
    )?;
    Ok(())
}
#[cfg(test)]
pub(super) fn remove_fixture(db: &Connection) {
    db.execute_batch(
        "DROP INDEX desktop_question_routes_child_submitted;
        DROP INDEX desktop_question_routes_parent_submitted;",
    )
    .unwrap();
}

/// Outcome metadata is retained even when no send original could be admitted.
pub(super) fn migrate_outcomes(tx: &Transaction<'_>) -> anyhow::Result<()> {
    tx.execute_batch(
        "ALTER TABLE desktop_question_routes ADD COLUMN record TEXT
        CHECK(record IS NULL OR (length(CAST(record AS BLOB))<=16384 AND json_valid(record)));
        UPDATE desktop_question_routes SET record=(SELECT checkpoint FROM desktop_deferred_sends d
        WHERE d.request_id=desktop_question_routes.request_id AND json_valid(d.checkpoint)
        AND json_extract(d.checkpoint,'$.kind')='route_question');
        CREATE TRIGGER desktop_question_route_record BEFORE UPDATE OF record ON desktop_question_routes BEGIN
        SELECT RAISE(ABORT,'immutable question route metadata'); END;
        CREATE INDEX desktop_question_routes_child_outcomes ON desktop_question_routes(child_session_id,created,request_id)
        WHERE state IN('submitted','refused','unknown');
        CREATE TABLE desktop_question_route_outcome_revision(id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL);
        INSERT INTO desktop_question_route_outcome_revision VALUES(1,0);
        CREATE TRIGGER desktop_question_route_failure_insert AFTER INSERT ON desktop_question_routes
        WHEN NEW.state IN('refused','unknown') BEGIN
        UPDATE desktop_question_route_outcome_revision SET revision=revision+1 WHERE id=1; END;
        CREATE TRIGGER desktop_question_route_failure_settle AFTER UPDATE OF state ON desktop_question_routes
        WHEN NEW.state IN('refused','unknown') BEGIN
        UPDATE desktop_question_route_outcome_revision SET revision=revision+1 WHERE id=1; END;",
    )?;
    Ok(())
}
#[cfg(test)]
pub(super) fn remove_outcomes_fixture(db: &Connection) {
    db.execute_batch(
        "DROP TRIGGER desktop_question_route_record;
        DROP TRIGGER desktop_question_route_failure_insert;
        DROP TRIGGER desktop_question_route_failure_settle;
        DROP TABLE desktop_question_route_outcome_revision;
        DROP INDEX desktop_question_routes_child_outcomes;
        ALTER TABLE desktop_question_routes DROP COLUMN record;",
    )
    .unwrap();
}

/// The two exact reads, each at most [`LIMIT`] rows of its own index.
pub(super) const CHILD_SQL: &str = "SELECT request_id,child_session_id,parent_session_id,created,state,record
    FROM desktop_question_routes INDEXED BY desktop_question_routes_child_outcomes
    WHERE child_session_id=?1 AND state IN('submitted','refused','unknown') ORDER BY created DESC,request_id DESC LIMIT ?2";
pub(super) const PARENT_SQL: &str = "SELECT request_id,child_session_id,parent_session_id,created,state,record
    FROM desktop_question_routes INDEXED BY desktop_question_routes_parent_submitted
    WHERE parent_session_id=?1 AND state='submitted' ORDER BY created DESC,request_id DESC LIMIT ?2";

struct Route {
    request_id: String,
    child: String,
    parent: String,
    created: i64,
    state: String,
    record: Option<String>,
}

fn newest(db: &Connection, sql: &str, session: &str) -> rusqlite::Result<Vec<Route>> {
    db.prepare_cached(sql)?
        .query_map(params![session, LIMIT as i64], |r| {
            Ok(Route {
                request_id: r.get(0)?,
                child: r.get(1)?,
                parent: r.get(2)?,
                created: r.get(3)?,
                state: r.get(4)?,
                record: r.get(5)?,
            })
        })?
        .collect()
}

/// The newest [`LIMIT`] submitted routes of `session`, newest first. A
/// route whose original is missing or does not name exactly this ledger
/// row is left out.
pub(crate) fn routes(db: &Connection, session: &str) -> anyhow::Result<Vec<Value>> {
    let mut rows = newest(db, CHILD_SQL, session)?;
    rows.extend(newest(db, PARENT_SQL, session)?);
    rows.sort_by(|a, b| (b.created, &b.request_id).cmp(&(a.created, &a.request_id)));
    rows.truncate(LIMIT);
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let record: Option<zerus_client_core::desktop::question_route::Record> = row
            .record
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok());
        let Some(record) = record else { continue };
        if !record.displayable()
            || record.request_id() != row.request_id
            || record.identity.session_id != row.child
            || record.parent.session_id != row.parent
            || (row.state == "submitted" && !record.valid())
        {
            continue;
        }
        if row.state == "submitted"
            && crate::desktop::deferred_send::route_record(db, &row.request_id)
                .ok()
                .flatten()
                .as_ref()
                != Some(&record)
        {
            continue;
        }
        let child = row.child == session;
        let mut line = json!({
            "request_id": row.request_id,
            "at": row.created,
            "role": if child { "child" } else { "parent" },
            "child": {"name": record.child.name, "reference": record.child.reference,
                "session_id": row.child},
            "parent": {"name": record.parent.session, "session_id": row.parent},
            "question": record.question,
        });
        if row.state != "submitted" {
            line["outcome"] = json!(if row.state == "refused" {
                "not_sent"
            } else {
                "uncertain"
            });
        }
        if !child {
            line["text"] = Value::String(record.text());
        }
        out.push(line);
    }
    Ok(out)
}

/// `{"op":"question_route_activity","request_id":UUID,"session_id":UUID}`:
/// exactly these keys and canonical UUIDs, else a closed refusal.
pub(in crate::desktop) fn request(db: &Connection, value: &Value) -> Value {
    let id = value["request_id"].as_str().unwrap_or("");
    let session = value["session_id"].as_str().unwrap_or("");
    let refused =
        |code: &str| json!({"request_id": id, "accepted": false, "durable": false, "code": code});
    let shape = value.as_object().is_some_and(|object| {
        object.len() == 3
            && value["op"] == OP
            && super::policy::canonical(id)
            && super::policy::canonical(session)
    });
    if !shape {
        return refused("invalid_question_route_activity");
    }
    match routes(db, session) {
        Ok(routes) => json!({"request_id": id, "accepted": true, "durable": false, "op": OP,
            "session_id": session, "routes": routes}),
        Err(_) => refused("question_route_store_refused"),
    }
}
