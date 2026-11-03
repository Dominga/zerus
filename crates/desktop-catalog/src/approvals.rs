//! The approval-phase index (zerus-5wmh.66.3.33): each stored catalog row
//! carries `approval`, set in the transaction that writes the row, so the
//! automatic-approval scan finds sessions waiting for approval without
//! walking the whole catalog. It is a discovery index only: the service still
//! decides eligibility from the row itself, a fresh native inspection and a
//! durable reservation. Because the flag belongs to the stored row version, a
//! resync (a new generation), a removal and a phase change need no separate
//! bookkeeping and leave nothing stale.
use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::Value;

/// Root schema 22: the flag and its indexes. Rows stored before it read NULL
/// until `backfill` derives them in bounded batches.
pub fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "ALTER TABLE desktop_catalog_rows ADD COLUMN approval INTEGER;
         CREATE INDEX desktop_catalog_approval ON desktop_catalog_rows(generation,id) WHERE approval=1;
         CREATE INDEX desktop_catalog_approval_pending ON desktop_catalog_rows(id) WHERE approval IS NULL;",
    )?;
    Ok(())
}

/// The row is in an approval phase: its own phase is `approval` or `input`,
/// or any subagent is waiting for approval. A superset of every row the
/// automatic-approval policy can accept (pinned by the service's tests).
pub fn approval_phase(row: &Value) -> bool {
    let waiting =
        |value: Option<&Value>, wanted: &str| value.and_then(Value::as_str) == Some(wanted);
    matches!(
        row.get("phase").and_then(Value::as_str),
        Some("approval" | "input")
    ) || row
        .get("subagents")
        .and_then(Value::as_object)
        .is_some_and(|children| {
            children.values().any(|child| {
                waiting(child.get("phase"), "approval")
                    || waiting(child.get("state"), "approval")
                    || waiting(child.get("display_state"), "approval")
                    || waiting(child.get("status"), "Needs approval")
            })
        })
}

/// Derive the flag of at most `limit` rows stored before schema 22, oldest
/// first, in one short transaction; 0 once none is left. Each call resumes
/// where the last one stopped, including after a restart.
pub fn backfill(db: &mut Connection, limit: usize) -> Result<usize> {
    let tx = db.transaction()?;
    let pending: Vec<(i64, String)> = {
        let mut query = tx.prepare(
            "SELECT id,json FROM desktop_catalog_rows WHERE approval IS NULL ORDER BY id LIMIT ?1",
        )?;
        let rows = query.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (id, json) in &pending {
        // A row that is not valid JSON is in no phase the scan could use.
        let flag = serde_json::from_str::<Value>(json).is_ok_and(|row| approval_phase(&row));
        tx.execute(
            "UPDATE desktop_catalog_rows SET approval=?2 WHERE id=?1 AND approval IS NULL",
            params![id, flag as i64],
        )?;
    }
    tx.commit()?;
    Ok(pending.len())
}

/// One stored row of the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: i64,
    pub key: String,
    pub json: String,
}

/// At most `limit` flagged rows of one generation after row id `after`, in id
/// order. Ids only grow and an update keeps its row's id, so a walk that
/// continues from the last id it saw stays stable while rows arrive.
pub fn page(db: &Connection, generation: i64, after: i64, limit: usize) -> Result<Vec<Row>> {
    let mut query = db.prepare(
        "SELECT id,key,json FROM desktop_catalog_rows
         WHERE generation=?1 AND approval=1 AND id>?2 ORDER BY id LIMIT ?3",
    )?;
    let rows = query.query_map(params![generation, after, limit as i64], |r| {
        Ok(Row {
            id: r.get(0)?,
            key: r.get(1)?,
            json: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests;
