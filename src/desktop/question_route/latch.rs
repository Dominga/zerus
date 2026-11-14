//! The person's latch on one child's question route (zerus-sa9qn.50,
//! design B). While a child's `person_off` is set, its questions never
//! route, whatever its host file says: each reservation reads the latch in
//! its own transaction beside the global switch and the host file. Only the
//! person's native UI permit (`Request::QuestionRouteChild`) or a phone
//! request the service verifies sets or clears it, by compare-and-swap.
//!
//! Boundary: the latch is person-only against an agent using commands (the
//! CLI, `hgs desktop request`, the connector). Like the global switch, it is
//! kept in the service Store, which the same user can edit directly; that is
//! outside this protection.
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use zerus_client_core::desktop::ui_requests::Request;

/// Request receipts kept for an uncertain result's lookup.
const RECEIPTS: i64 = 4096;
/// Latched children kept; beyond this a new latch is refused, never dropped.
pub(super) const CHILDREN: i64 = 65_536;
pub(super) const OP: &str = "question_route_child";
pub(super) const RESULT_OP: &str = "question_route_child_result";
pub(super) const STATUS_OP: &str = "question_route_child_status";

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE desktop_question_route_children(
        session_id TEXT PRIMARY KEY CHECK(length(session_id)=36),
        person_off INTEGER NOT NULL CHECK(person_off IN(0,1)),
        revision INTEGER NOT NULL CHECK(revision>=0),
        decided_at INTEGER NOT NULL CHECK(decided_at>=0));
        CREATE TRIGGER desktop_question_route_child_kept BEFORE DELETE ON desktop_question_route_children BEGIN
        SELECT RAISE(ABORT,'a question route latch is kept'); END;
        CREATE TABLE desktop_question_route_child_requests(
        request_id TEXT PRIMARY KEY CHECK(length(request_id)=36),
        session_id TEXT NOT NULL CHECK(length(session_id)=36),
        person_off INTEGER NOT NULL CHECK(person_off IN(0,1)),
        revision INTEGER NOT NULL CHECK(revision>=0),
        applied INTEGER NOT NULL CHECK(applied IN(0,1)),
        created INTEGER NOT NULL);
        CREATE INDEX desktop_question_route_child_requests_age
        ON desktop_question_route_child_requests(created,request_id);
        CREATE TRIGGER desktop_question_route_child_receipt BEFORE UPDATE ON desktop_question_route_child_requests BEGIN
        SELECT RAISE(ABORT,'a question route latch receipt is immutable'); END;",
    )?;
    Ok(())
}

/// Once any verified v2 statement reaches a latch, legacy clock ordering
/// can never be used on it again, including by another phone.
pub(super) fn migrate_order(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "ALTER TABLE desktop_question_route_children ADD COLUMN revision_guarded INTEGER NOT NULL DEFAULT 0 CHECK(revision_guarded IN(0,1));
        CREATE TRIGGER desktop_question_route_child_fenced BEFORE UPDATE OF revision_guarded ON desktop_question_route_children
        WHEN OLD.revision_guarded=1 AND NEW.revision_guarded<>1 BEGIN
        SELECT RAISE(ABORT,'a question route revision fence is kept'); END;",
    )?;
    Ok(())
}
#[cfg(test)]
pub(super) fn remove_order_fixture(db: &Connection) {
    db.execute_batch("DROP TRIGGER desktop_question_route_child_fenced; ALTER TABLE desktop_question_route_children DROP COLUMN revision_guarded;").unwrap();
}

#[cfg(test)]
pub(super) fn remove_fixture(db: &Connection) {
    db.execute_batch(
        "DROP TABLE desktop_question_route_children; DROP TABLE desktop_question_route_child_requests;",
    )
    .unwrap();
}

/// The child's latch, its revision and the time of the person's last
/// decision (seconds), and whether v2 fenced it. An absent row is
/// `(false, 0, 0, false)`. A corrupt row is an error, never "not latched".
fn decision(db: &Connection, session_id: &str) -> Result<(bool, i64, i64, bool)> {
    let row: Option<(i64, i64, i64, i64)> = db
        .query_row(
            "SELECT person_off,revision,decided_at,revision_guarded FROM desktop_question_route_children
             WHERE session_id=?1",
            [session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    match row {
        None => Ok((false, 0, 0, false)),
        Some((off, revision, decided, fenced)) => {
            ensure!(
                matches!(off, 0 | 1) && revision >= 0 && decided >= 0 && matches!(fenced, 0 | 1),
                "question route latch changed"
            );
            Ok((off == 1, revision, decided, fenced == 1))
        }
    }
}
/// The child's latch and its revision.
pub(super) fn state(db: &Connection, session_id: &str) -> Result<(bool, i64)> {
    let (off, revision, _, _) = decision(db, session_id)?;
    Ok((off, revision))
}

/// Desktop and v2 phone decisions compare the computer's latch revision.
/// Legacy phones retain time ordering only until this latch sees v2.
pub(super) enum Guard {
    Revision(i64),
    PhoneRevision(i64),
    IssuedAt(i64),
}

/// A v2 phone statement signs the computer-issued latch revision.
/// Legacy statements are accepted only on latches that never saw v2.
impl Guard {
    fn phone_revision(&self) -> Option<i64> {
        match self {
            Self::PhoneRevision(revision) => Some(*revision),
            _ => None,
        }
    }
}

/// Whether the person latched this child off, read in the caller's
/// transaction.
pub(super) fn latched(db: &Connection, session_id: &str) -> Result<bool> {
    Ok(state(db, session_id)?.0)
}

fn reply(id: &str, session_id: &str, person_off: bool, revision: i64, applied: bool) -> Value {
    json!({"request_id": id, "op": OP, "accepted": true, "durable": true, "session_id": session_id,
        "person_off": person_off, "revision": revision, "applied": applied, "code": ""})
}
fn refusal(id: &str, code: &str) -> Value {
    json!({"request_id": id, "op": OP, "accepted": false, "durable": false, "code": code})
}

fn receipt(db: &Connection, id: &str) -> Result<Option<Value>> {
    Ok(db
        .query_row(
            "SELECT session_id,person_off,revision,applied FROM desktop_question_route_child_requests
             WHERE request_id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?
        .map(|(session, off, revision, applied)| reply(id, &session, off == 1, revision, applied == 1)))
}

fn fence(tx: &Transaction<'_>, session_id: &str) -> Result<bool> {
    if tx.execute(
        "UPDATE desktop_question_route_children SET revision_guarded=1 WHERE session_id=?1",
        [session_id],
    )? == 1
    {
        return Ok(true);
    }
    let kept: i64 = tx.query_row(
        "SELECT count(*) FROM desktop_question_route_children",
        [],
        |r| r.get(0),
    )?;
    if kept >= CHILDREN {
        return Ok(false);
    }
    tx.execute("INSERT INTO desktop_question_route_children(session_id,person_off,revision,decided_at,revision_guarded) VALUES(?1,0,0,0,1)", [session_id])?;
    Ok(true)
}

/// Sets the child's latch to `person_off` for request `id`, once, as
/// `guard` orders it. The same request again returns its receipt; another
/// value or child under a used request is refused. A stale revision is the
/// closed `conflict` refusal with the current value. A stale v2 revision,
/// or a legacy issue time not after the last decision, is `stale_decision`; neither
/// changes the latch. A verified v2 request durably fences the latch even
/// on a conflict. Every accepted new decision advances its revision, even
/// when the value was already so. Replay returns the original receipt.
pub(super) fn set(
    db: &mut Connection,
    id: &str,
    session_id: &str,
    person_off: bool,
    guard: Guard,
    at: i64,
) -> Result<Value> {
    ensure!(
        super::policy::canonical(id) && super::policy::canonical(session_id),
        "invalid question route latch request"
    );
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(old) = receipt(&tx, id)? {
        ensure!(
            old["session_id"] == session_id && old["person_off"] == person_off,
            "question route latch request reused"
        );
        return Ok(old);
    }
    let (now, revision, decided, fenced) = decision(&tx, session_id)?;
    if guard.phone_revision().is_some() && !fence(&tx, session_id)? {
        return Ok(refusal(id, "capacity"));
    }
    let decided = match guard {
        Guard::Revision(expected) => {
            if expected != revision {
                let mut conflict = refusal(id, "conflict");
                conflict["person_off"] = json!(now);
                conflict["revision"] = json!(revision);
                return Ok(conflict);
            }
            decided.max(at.max(0) / 1000)
        }
        Guard::PhoneRevision(expected) => {
            if expected != revision {
                tx.commit()?;
                return Ok(refusal(id, "stale_decision"));
            }
            // Audit/legacy metadata only; never consulted for v2 ordering.
            decided.max(at.max(0) / 1000)
        }
        Guard::IssuedAt(issued) => {
            if fenced {
                return Ok(refusal(id, "upgrade_required"));
            }
            if issued <= decided {
                return Ok(refusal(id, "stale_decision"));
            }
            issued
        }
    };
    let applied = now != person_off;
    let next = revision
        .checked_add(1)
        .filter(|r| *r < i64::MAX)
        .ok_or_else(|| anyhow::anyhow!("question route latch revisions exhausted"))?;
    let present = tx
        .query_row(
            "SELECT 1 FROM desktop_question_route_children WHERE session_id=?1",
            [session_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if present {
        ensure!(
            tx.execute(
                "UPDATE desktop_question_route_children SET person_off=?1,revision=?2,decided_at=?3
                 WHERE session_id=?4 AND revision=?5",
                params![person_off, next, decided, session_id, revision],
            )? == 1,
            "question route latch conflict"
        );
    } else {
        let kept: i64 = tx.query_row(
            "SELECT count(*) FROM desktop_question_route_children",
            [],
            |r| r.get(0),
        )?;
        if kept >= CHILDREN {
            return Ok(refusal(id, "capacity"));
        }
        tx.execute(
            "INSERT INTO desktop_question_route_children(session_id,person_off,revision,decided_at)
             VALUES(?1,?2,?3,?4)",
            params![session_id, person_off, next, decided],
        )?;
    }
    let revision = next;
    let kept: i64 = tx.query_row(
        "SELECT count(*) FROM desktop_question_route_child_requests",
        [],
        |r| r.get(0),
    )?;
    if kept >= RECEIPTS {
        tx.execute(
            "DELETE FROM desktop_question_route_child_requests WHERE request_id IN
             (SELECT request_id FROM desktop_question_route_child_requests
              INDEXED BY desktop_question_route_child_requests_age ORDER BY created,request_id LIMIT ?1)",
            [kept - RECEIPTS + 1],
        )?;
    }
    tx.execute(
        "INSERT INTO desktop_question_route_child_requests(request_id,session_id,person_off,revision,
         applied,created) VALUES(?1,?2,?3,?4,?5,?6)",
        params![id, session_id, person_off, revision, applied, at],
    )?;
    tx.commit()?;
    Ok(reply(id, session_id, person_off, revision, applied))
}

/// After an accepted latch change, the child's host file follows it and
/// mirrors it for display (marker `session-question-routing-v2`). Off holds
/// through the latch alone, so a failed write here never routes a question;
/// on needs the host as well, and until it is written nothing routes. The
/// reply is the latch receipt alone, so a replay answers the same.
fn follow(state: &std::path::Path, session_id: &str, reply: &Value) {
    if reply["accepted"] == true {
        let _ = crate::state::question_routing::set_for_person(
            state,
            session_id,
            reply["person_off"] == true,
        );
    }
}

/// The person's native request (`question_route_child`), from either the
/// passive or the background frontend lane. The child must be the one
/// visible local row with this session ID, at the request's run and
/// conversation.
pub(in crate::desktop) fn frontend(
    db: &mut Connection,
    state: &std::path::Path,
    authority: crate::desktop::ui_authority::Validated,
) -> Value {
    let id = authority.id().to_owned();
    let Ok(Request::QuestionRouteChild {
        session_id,
        run_id,
        conversation_id,
        person_off,
        expected_revision,
        ..
    }) = authority.into_request()
    else {
        return refusal(&id, "frontend_identity_expired");
    };
    if !super::policy::canonical(&id) {
        return refusal(&id, "invalid_request_id");
    }
    match super::session_row(db, &session_id) {
        Ok(Some(row)) if row["run_id"] == run_id && row["conversation_id"] == conversation_id => {}
        Ok(Some(_)) => return refusal(&id, "stale_identity"),
        Ok(None) => return refusal(&id, "not_found"),
        Err(_) => return refusal(&id, "question_route_store_refused"),
    }
    let Ok(expected) = i64::try_from(expected_revision) else {
        return refusal(&id, "invalid_request");
    };
    let reply = match set(
        db,
        &id,
        &session_id,
        person_off,
        Guard::Revision(expected),
        chrono::Utc::now().timestamp_millis(),
    ) {
        Ok(reply) => reply,
        Err(_) => return refusal(&id, "question_route_store_refused"),
    };
    follow(state, &session_id, &reply);
    reply
}

/// The read-only lookup of one request's receipt
/// (`question_route_child_result`).
pub(in crate::desktop) fn result_request(db: &Connection, value: &Value) -> Value {
    let id = value["request_id"].as_str().unwrap_or("");
    let refused =
        |code: &str| json!({"request_id": id, "accepted": false, "durable": false, "code": code});
    let original = value["original_request_id"].as_str().unwrap_or("");
    let shape = value.as_object().is_some_and(|object| {
        object.len() == 3
            && value["op"] == RESULT_OP
            && super::policy::canonical(id)
            && super::policy::canonical(original)
    });
    if !shape {
        return refused("invalid_question_route_child_request");
    }
    match receipt(db, original) {
        Ok(result) => json!({"request_id": id, "accepted": true, "durable": false, "op": RESULT_OP,
            "original_request_id": original, "found": result.is_some(), "result": result}),
        Err(_) => refused("question_route_store_refused"),
    }
}

/// The read-only latch of one child (`question_route_child_status`), for
/// the person's next compare-and-swap.
pub(in crate::desktop) fn status_request(db: &Connection, value: &Value) -> Value {
    let id = value["request_id"].as_str().unwrap_or("");
    let session = value["session_id"].as_str().unwrap_or("");
    let refused =
        |code: &str| json!({"request_id": id, "accepted": false, "durable": false, "code": code});
    let scoped = value.get("run_id").is_some() || value.get("conversation_id").is_some();
    let shape = value.as_object().is_some_and(|object| {
        (object.len() == 3 && !scoped
            || object.len() == 5
                && scoped
                && value["run_id"].as_str().is_some_and(|s| !s.is_empty())
                && value["conversation_id"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty()))
            && value["op"] == STATUS_OP
            && super::policy::canonical(id)
            && super::policy::canonical(session)
    });
    if !shape {
        return refused("invalid_question_route_child_request");
    }
    if scoped {
        match super::session_row(db, session) {
            Ok(Some(row))
                if row["run_id"] == value["run_id"]
                    && row["conversation_id"] == value["conversation_id"] => {}
            Ok(Some(_)) => return refused("stale_identity"),
            Ok(None) => return refused("not_found"),
            Err(_) => return refused("question_route_store_refused"),
        }
    }
    match state(db, session) {
        Ok((person_off, revision)) => {
            let mut reply = json!({"request_id": id, "accepted": true, "durable": false,
                "op": STATUS_OP, "session_id": session, "person_off": person_off, "revision": revision});
            if scoped {
                reply["run_id"] = value["run_id"].clone();
                reply["conversation_id"] = value["conversation_id"].clone();
            }
            reply
        }
        Err(_) => refused("question_route_store_refused"),
    }
}

pub(super) const PERSON_OP: &str = "question_route_person";

/// A phone's latch statement (zerus-sa9qn.50), as the connector forwards
/// it. Its fields are the shared crypto crate's `RoutePersonStatement`.
#[derive(serde::Deserialize, serde::Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Statement {
    workspace_id: String,
    device_id: String,
    gateway_id: String,
    session_id: String,
    run_id: String,
    conversation_id: String,
    person_off: bool,
    request_id: String,
    issued_at: u64,
    expires_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_revision: Option<u64>,
}

/// `{"op":"question_route_person","statement":{...},"signature":"..."}`:
/// a phone's request through the connector. It applies only what this
/// service's own run of the connector's verifier (`verify`) confirms: a
/// current phone of this workspace signed exactly this statement for this
/// computer, unexpired. The latch is set to the statement's value once per
/// `request_id`, then the child's host file follows (`state`, the CLI state
/// root), so the phone needs one request. The forwarder grants nothing.
pub(in crate::desktop) fn person_request(
    db: &mut Connection,
    state: &std::path::Path,
    value: &Value,
    verify: impl FnOnce(&Value) -> Result<Value>,
) -> Value {
    let id = value["statement"]["request_id"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let shape = value.as_object().is_some_and(|o| {
        o.len() == 3 && value["op"] == PERSON_OP && value["signature"].is_string()
    }) && value["statement"]
        .get("expected_revision")
        .is_none_or(|r| r.as_u64().is_some_and(|r| r < i64::MAX as u64));
    let Some(statement) = shape
        .then(|| serde_json::from_value::<Statement>(value["statement"].clone()).ok())
        .flatten()
        .filter(|s| {
            super::policy::canonical(&s.request_id) && super::policy::canonical(&s.session_id)
        })
    else {
        return refusal(&id, "invalid_request");
    };
    // A request already applied returns its receipt.
    match receipt(db, &id) {
        Ok(Some(old)) => {
            return if old["session_id"] == statement.session_id
                && old["person_off"] == statement.person_off
            {
                old
            } else {
                refusal(&id, "invalid_request")
            }
        }
        Ok(None) => {}
        Err(_) => return refusal(&id, "question_route_store_refused"),
    }
    let input = json!({"statement": statement, "signature": value["signature"]});
    match verify(&input) {
        Ok(reply) if reply["ok"] == true && reply["verified"] == input["statement"] => {}
        _ => return refusal(&id, "not_verified"),
    }
    match super::session_row(db, &statement.session_id) {
        Ok(Some(row))
            if row["run_id"] == statement.run_id
                && row["conversation_id"] == statement.conversation_id => {}
        Ok(row) => {
            // Even a verified statement for an absent child or its previous
            // run must prevent a later downgrade to clock ordering. A new
            // fence keeps the default value and is subject to CHILDREN.
            if statement.expected_revision.is_some() {
                let pinned = (|| -> Result<bool> {
                    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                    let kept = fence(&tx, &statement.session_id)?;
                    tx.commit()?;
                    Ok(kept)
                })();
                match pinned {
                    Ok(true) => {}
                    Ok(false) => return refusal(&id, "capacity"),
                    Err(_) => return refusal(&id, "question_route_store_refused"),
                }
            }
            return refusal(
                &id,
                if row.is_some() {
                    "stale_identity"
                } else {
                    "not_found"
                },
            );
        }
        Err(_) => return refusal(&id, "question_route_store_refused"),
    }
    let guard = if let Some(revision) = statement.expected_revision {
        Guard::PhoneRevision(revision as i64)
    } else if let Ok(issued) = i64::try_from(statement.issued_at) {
        Guard::IssuedAt(issued)
    } else {
        return refusal(&id, "invalid_request");
    };
    let reply = match set(
        db,
        &id,
        &statement.session_id,
        statement.person_off,
        guard,
        chrono::Utc::now().timestamp_millis(),
    ) {
        Ok(reply) => reply,
        Err(_) => return refusal(&id, "question_route_store_refused"),
    };
    follow(state, &statement.session_id, &reply);
    reply
}
