//! The custody-input records of reference originals (Store schema 27), with
//! their own bounded storage charge, separate from the operation charge.
//!
//! One record per prepared input file, keyed by the original request ID and
//! never deleted. States:
//! - `reserved`: capacity reserved before any preparation; charged.
//! - `held`: admitted with its original's row; its file identity is recorded;
//!   charged until the original's resources drained and the file is removed.
//! - `removed`: the exact file was removed after that drain; uncharged.
//! - `failed`: that removal failed; visible and still charged.
//! - `released`: refused before admission, its file never created or removed
//!   exactly; uncharged.
//! - `orphaned`: a crash after its file was published but before admission;
//!   the exact file is kept for review and stays charged.
//!
//! Nothing here reads an input file; lookups never depend on one.
use anyhow::{ensure, Result};
use rusqlite::{params, OptionalExtension, Transaction};

// Merge-order allocation, like the leaf's own schema.
pub(super) const SCHEMA: i64 = 27;
/// The most custody input bytes held at once, in-flight reservations and
/// orphans included.
pub(super) const BYTES: i64 = 64 << 20;
/// The most charged records at once.
pub(super) const ROWS: i64 = 64;

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE desktop_task_message_input_budget(
        id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 64),
        bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 67108864));
        INSERT INTO desktop_task_message_input_budget VALUES(1,0,0);
        CREATE TABLE desktop_task_message_inputs(
        request_id TEXT PRIMARY KEY CHECK(length(request_id)=36),
        bytes INTEGER NOT NULL CHECK(bytes BETWEEN 1 AND 67108864),
        input_sha256 TEXT NOT NULL CHECK(length(input_sha256)=64),
        device INTEGER,inode INTEGER,
        state TEXT NOT NULL CHECK(state IN('reserved','held','removed','failed','released','orphaned')),
        created INTEGER NOT NULL,
        CHECK((device IS NULL)=(inode IS NULL)),
        CHECK(state NOT IN('held','removed','failed') OR device IS NOT NULL));
        CREATE INDEX desktop_task_message_inputs_open ON desktop_task_message_inputs(state,request_id)
        WHERE state NOT IN('removed','released');
        CREATE TRIGGER desktop_task_message_input_original BEFORE UPDATE OF request_id,bytes,input_sha256,created
        ON desktop_task_message_inputs BEGIN
        SELECT RAISE(ABORT,'immutable task message input'); END;
        CREATE TRIGGER desktop_task_message_input_identity BEFORE UPDATE OF device,inode ON desktop_task_message_inputs
        WHEN OLD.device IS NOT NULL BEGIN
        SELECT RAISE(ABORT,'immutable task message input identity'); END;
        CREATE TRIGGER desktop_task_message_input_preserve BEFORE DELETE ON desktop_task_message_inputs BEGIN
        SELECT RAISE(ABORT,'task message inputs are retained'); END;",
    )?;
    Ok(())
}

/// One record's state, size and recorded file identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Record {
    pub(super) state: String,
    pub(super) bytes: u64,
    pub(super) input_sha256: String,
    pub(super) identity: Option<(u64, u64)>,
}

pub(super) fn load(db: &rusqlite::Connection, request: &str) -> Result<Option<Record>> {
    Ok(db
        .query_row(
            "SELECT state,bytes,device,inode,input_sha256 FROM desktop_task_message_inputs WHERE request_id=?1",
            [request],
            |r| {
                let identity: Option<(i64, i64)> = match (r.get(2)?, r.get(3)?) {
                    (Some(d), Some(i)) => Some((d, i)),
                    _ => None,
                };
                Ok(Record {
                    state: r.get(0)?,
                    bytes: r.get::<_, i64>(1)? as u64,
                    input_sha256: r.get(4)?,
                    identity: identity.map(|(d, i)| (d as u64, i as u64)),
                })
            },
        )
        .optional()?)
}

fn charge(tx: &Transaction<'_>, rows: i64, bytes: i64) -> Result<()> {
    ensure!(
        tx.execute(
            "UPDATE desktop_task_message_input_budget SET rows=rows+?1,bytes=bytes+?2
             WHERE id=1 AND rows+?1 BETWEEN 0 AND ?3 AND bytes+?2 BETWEEN 0 AND ?4",
            params![rows, bytes, ROWS, BYTES],
        )? == 1,
        "task message input budget unavailable"
    );
    Ok(())
}

fn transition(tx: &Transaction<'_>, request: &str, from: &[&str], to: &str) -> Result<Record> {
    let record = load(tx, request)?.ok_or_else(|| anyhow::anyhow!("task message input missing"))?;
    ensure!(
        from.contains(&record.state.as_str()),
        "task message input state changed"
    );
    tx.execute(
        "UPDATE desktop_task_message_inputs SET state=?2 WHERE request_id=?1",
        params![request, to],
    )?;
    Ok(record)
}

/// Reserves the exact encoded input size before any preparation. A request
/// ID that already has a record never gets another.
pub(super) fn reserve(
    tx: &Transaction<'_>,
    request: &str,
    bytes: u64,
    input_sha256: &str,
    now: i64,
) -> Result<()> {
    ensure!(load(tx, request)?.is_none(), "task message input exists");
    let size = i64::try_from(bytes)?;
    charge(tx, 1, size)?;
    tx.execute(
        "INSERT INTO desktop_task_message_inputs(request_id,bytes,input_sha256,state,created)
         VALUES(?1,?2,?3,'reserved',?4)",
        params![request, size, input_sha256, now],
    )?;
    Ok(())
}

/// The reserved input was published and admitted with its row.
pub(super) fn hold(tx: &Transaction<'_>, request: &str, identity: (u64, u64)) -> Result<()> {
    // The state and the identity change together: a held record always
    // names its file.
    ensure!(
        tx.execute(
            "UPDATE desktop_task_message_inputs SET state='held',device=?2,inode=?3
             WHERE request_id=?1 AND state='reserved'",
            params![request, identity.0 as i64, identity.1 as i64],
        )? == 1,
        "task message input state changed"
    );
    Ok(())
}

/// A refusal before admission: nothing of this reservation remains.
pub(super) fn release(tx: &Transaction<'_>, request: &str) -> Result<()> {
    let record = transition(tx, request, &["reserved"], "released")?;
    charge(tx, -1, -(record.bytes as i64))
}

/// The exact file was removed after its original drained; also a retried
/// removal that failed before.
pub(super) fn removed(tx: &Transaction<'_>, request: &str) -> Result<()> {
    let record = transition(tx, request, &["held", "failed"], "removed")?;
    charge(tx, -1, -(record.bytes as i64))
}

/// The removal failed: the record stays visible and charged.
pub(super) fn failed(tx: &Transaction<'_>, request: &str) -> Result<()> {
    transition(tx, request, &["held", "failed"], "failed").map(drop)
}

/// After a restart, a reservation whose file exists was published but never
/// admitted: kept, charged and reviewable. One without a file is released.
pub(super) fn recover_reserved(tx: &Transaction<'_>, request: &str, published: bool) -> Result<()> {
    if published {
        transition(tx, request, &["reserved"], "orphaned").map(drop)
    } else {
        release(tx, request)
    }
}

// Every open-record query states the open index's own predicate: SQLite uses
// a partial index only for a WHERE clause that contains its terms, and
// without it each read would scan the retained terminal history.

/// Up to `?2` records in state `?1`, oldest first.
pub(super) const IN_STATE: &str = "SELECT request_id FROM desktop_task_message_inputs
     WHERE state NOT IN('removed','released') AND state=?1
     ORDER BY created,request_id LIMIT ?2";
/// Up to `?1` held records whose original drained, oldest first.
pub(super) const HELD_DRAINED: &str = "SELECT i.request_id FROM desktop_task_message_inputs i
     JOIN desktop_task_messages m ON m.request_id=i.request_id
     WHERE i.state NOT IN('removed','released') AND i.state='held' AND m.drained=1
     ORDER BY i.created,i.request_id LIMIT ?1";
/// Up to `?3` reviewable records after the cursor `(?1, ?2)`.
pub(super) const REVIEWABLE: &str =
    "SELECT request_id,state,bytes,input_sha256,created,device,inode
     FROM desktop_task_message_inputs
     WHERE state NOT IN('removed','released') AND state IN('orphaned','failed')
       AND (?1 IS NULL OR created>?1 OR (created=?1 AND request_id>?2))
     ORDER BY created,request_id LIMIT ?3";

/// Up to `limit` records in `state`, oldest first.
pub(super) fn in_state(db: &rusqlite::Connection, state: &str, limit: u32) -> Result<Vec<String>> {
    let mut query = db.prepare(IN_STATE)?;
    let rows = query.query_map(params![state, limit], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Up to `limit` held records whose original drained, oldest first.
pub(super) fn held_drained(db: &rusqlite::Connection, limit: u32) -> Result<Vec<String>> {
    let mut query = db.prepare(HELD_DRAINED)?;
    let rows = query.query_map(params![limit], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// One record a person may review (zerus-pi2k9): `orphaned` or `failed`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Reviewable {
    pub(super) request_id: String,
    pub(super) state: String,
    pub(super) bytes: u64,
    pub(super) input_sha256: String,
    pub(super) created: i64,
    pub(super) identity: Option<(u64, u64)>,
}

/// Up to `limit` reviewable records after `after` (creation time, then
/// request ID). The search uses the open-state index; at most the budget's
/// 64 charged records are sorted.
pub(super) fn reviewable(
    db: &rusqlite::Connection,
    after: Option<(i64, &str)>,
    limit: u32,
) -> Result<Vec<Reviewable>> {
    let (created, request) = after.map_or((None, ""), |(c, r)| (Some(c), r));
    let mut query = db.prepare(REVIEWABLE)?;
    let rows = query.query_map(params![created, request, limit], |r| {
        let identity: Option<(i64, i64)> = match (r.get(5)?, r.get(6)?) {
            (Some(d), Some(i)) => Some((d, i)),
            _ => None,
        };
        Ok(Reviewable {
            request_id: r.get(0)?,
            state: r.get(1)?,
            bytes: r.get::<_, i64>(2)? as u64,
            input_sha256: r.get(3)?,
            created: r.get(4)?,
            identity: identity.map(|(d, i)| (d as u64, i as u64)),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// One exact reviewable record, by request ID.
pub(super) fn reviewable_one(
    db: &rusqlite::Connection,
    request: &str,
) -> Result<Option<Reviewable>> {
    Ok(db
        .query_row(
            "SELECT request_id,state,bytes,input_sha256,created,device,inode
             FROM desktop_task_message_inputs WHERE request_id=?1",
            [request],
            |r| {
                let identity: Option<(i64, i64)> = match (r.get(5)?, r.get(6)?) {
                    (Some(d), Some(i)) => Some((d, i)),
                    _ => None,
                };
                Ok(Reviewable {
                    request_id: r.get(0)?,
                    state: r.get(1)?,
                    bytes: r.get::<_, i64>(2)? as u64,
                    input_sha256: r.get(3)?,
                    created: r.get(4)?,
                    identity: identity.map(|(d, i)| (d as u64, i as u64)),
                })
            },
        )
        .optional()?)
}

/// A review removed this record's exact observed object: `removed`,
/// uncharged. An orphaned record had no identity, and records the object
/// this review observed and removed, never a recovered original identity.
pub(super) fn reviewed_removed(
    tx: &Transaction<'_>,
    request: &str,
    from: &str,
    observed: (u64, u64),
) -> Result<()> {
    let record = load(tx, request)?.ok_or_else(|| anyhow::anyhow!("task message input missing"))?;
    ensure!(
        record.state == from && ["orphaned", "failed"].contains(&from),
        "task message input state changed"
    );
    match record.identity {
        Some(identity) => {
            ensure!(identity == observed, "task message input identity changed");
            ensure!(
                tx.execute(
                    "UPDATE desktop_task_message_inputs SET state='removed' WHERE request_id=?1 AND state=?2",
                    params![request, from],
                )? == 1,
                "task message input state changed"
            );
        }
        None => {
            ensure!(
                tx.execute(
                    "UPDATE desktop_task_message_inputs SET state='removed',device=?3,inode=?4
                     WHERE request_id=?1 AND state=?2 AND device IS NULL",
                    params![request, from, observed.0 as i64, observed.1 as i64],
                )? == 1,
                "task message input state changed"
            );
        }
    }
    charge(tx, -1, -(record.bytes as i64))
}

/// A review found this record's slot absent and no copy elsewhere: an
/// accounting release, `released` and uncharged. A failed record keeps its
/// recorded identity.
pub(super) fn reviewed_released(tx: &Transaction<'_>, request: &str, from: &str) -> Result<()> {
    ensure!(
        ["orphaned", "failed"].contains(&from),
        "task message input state changed"
    );
    let record = transition(tx, request, &[from], "released")?;
    charge(tx, -1, -(record.bytes as i64))
}

/// The charged records and bytes, for status.
pub(super) fn totals(db: &rusqlite::Connection) -> Result<(u64, u64)> {
    Ok(db.query_row(
        "SELECT rows,bytes FROM desktop_task_message_input_budget WHERE id=1",
        [],
        |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
    )?)
}
