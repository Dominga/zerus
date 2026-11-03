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

/// Derive the flag of rows stored before schema 22, oldest first, in one
/// short transaction: at most `limit` rows and, past the first, `bytes` of
/// their JSON, which is read one row at a time; 0 once none is left. Each
/// call resumes where the last one stopped, including after a restart.
pub fn backfill(db: &mut Connection, limit: usize, bytes: usize) -> Result<usize> {
    let tx = db.transaction()?;
    let mut derived: Vec<(i64, bool)> = Vec::new();
    {
        let mut query = tx.prepare(
            "SELECT id,json FROM desktop_catalog_rows WHERE approval IS NULL ORDER BY id LIMIT ?1",
        )?;
        let mut rows = query.query([limit as i64])?;
        let mut read = 0usize;
        while read < bytes || derived.is_empty() {
            let Some(row) = rows.next()? else { break };
            let id: i64 = row.get(0)?;
            let json = row.get_ref(1)?.as_str()?;
            read = read.saturating_add(json.len());
            // A row that is not valid JSON is in no phase the scan could use.
            let flag = serde_json::from_str::<Value>(json).is_ok_and(|row| approval_phase(&row));
            derived.push((id, flag));
        }
    }
    for (id, flag) in &derived {
        tx.execute(
            "UPDATE desktop_catalog_rows SET approval=?2 WHERE id=?1 AND approval IS NULL",
            params![id, *flag as i64],
        )?;
    }
    tx.commit()?;
    Ok(derived.len())
}

/// One stored row of the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: i64,
    pub key: String,
    pub json: String,
}

/// The highest id of a flagged row of one generation: the end of a pass that
/// starts now. Rows that arrive later belong to the next pass.
pub fn pass_end(db: &Connection, generation: i64) -> Result<Option<i64>> {
    Ok(db.query_row(
        "SELECT max(id) FROM desktop_catalog_rows WHERE generation=?1 AND approval=1",
        [generation],
        |r| r.get(0),
    )?)
}

/// At most `limit` flagged rows of one generation after row id `after` and up
/// to `end`, in id order. Ids only grow and an update keeps its row's id, so
/// a walk that continues from the last id it saw stays stable while rows
/// arrive, and a pass ending at its captured `end` cannot be outrun by them.
pub fn page(
    db: &Connection,
    generation: i64,
    after: i64,
    end: i64,
    limit: usize,
) -> Result<Vec<Row>> {
    let mut query = db.prepare(
        "SELECT id,key,json FROM desktop_catalog_rows
         WHERE generation=?1 AND approval=1 AND id>?2 AND id<=?3 ORDER BY id LIMIT ?4",
    )?;
    let rows = query.query_map(params![generation, after, end, limit as i64], |r| {
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
