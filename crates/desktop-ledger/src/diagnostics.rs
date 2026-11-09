//! Bounded, display-only failure summaries. No identities, payloads or replay.
use super::{State, RECEIPT_BYTES};
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};

const IDENTITY_BYTES: usize = 4096;
const LATEST: &str = "SELECT a.seq,a.recorded_at,
    CASE WHEN length(CAST(i.identity AS BLOB)) BETWEEN 1 AND ?2 THEN i.identity END,
    CASE WHEN length(CAST(a.receipt AS BLOB)) BETWEEN 1 AND ?3 THEN a.receipt END
    FROM desktop_attempts a INDEXED BY desktop_attempt_state
    LEFT JOIN desktop_intents i ON i.id=a.intent_id
    WHERE a.state=?1 ORDER BY a.seq DESC LIMIT 1";

/// Root owns schema versions. Earlier records have no recorded wall-clock time;
/// migration neither scans receipts nor invents historical timestamps.
pub fn migrate(tx: &Transaction<'_>) -> Result<()> {
    let mut query = tx.prepare("PRAGMA table_info(desktop_attempts)")?;
    let columns = query.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    for column in columns {
        let (name, kind) = column?;
        if name == "recorded_at" {
            ensure!(kind == "INTEGER", "invalid receipt recording column");
            return Ok(());
        }
    }
    drop(query);
    tx.execute_batch(
        "ALTER TABLE desktop_attempts ADD COLUMN recorded_at INTEGER CHECK(recorded_at>=0)",
    )?;
    Ok(())
}

fn latest(db: &Connection, state: State) -> Result<Value> {
    type Row = (i64, Option<i64>, Option<String>, Option<String>);
    let row: Option<Row> = db
        .query_row(
            LATEST,
            params![state.name(), IDENTITY_BYTES as i64, RECEIPT_BYTES as i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((seq, recorded_at, identity, receipt)) = row else {
        return Ok(Value::Null);
    };
    let identity = identity.and_then(|v| serde_json::from_str::<Value>(&v).ok());
    let kind = identity
        .as_ref()
        .and_then(|v| v["kind"].as_str())
        .filter(|kind| matches!(*kind, "post" | "withdrawal" | "badge" | "presence"));
    let receipt = receipt.and_then(|v| serde_json::from_str::<Value>(&v).ok());
    let code = receipt
        .as_ref()
        .and_then(|v| {
            let outcome = &v["outcome"];
            (outcome["state"] == state.name())
                .then(|| outcome["code"].as_str())
                .flatten()
        })
        .filter(|code| match state {
            State::Refused => matches!(
                *code,
                "not_ready"
                    | "cancelled"
                    | "invalid_command"
                    | "unsupported"
                    | "capacity"
                    | "permission_denied"
                    | "getter_failed"
            ),
            State::Unknown => matches!(
                *code,
                "transport" | "deadline" | "native_error" | "host_lost"
            ),
            _ => false,
        });
    Ok(json!({"seq":seq,"kind":kind,"code":code,
        "recorded_at":recorded_at.filter(|time| *time>=0),
        "details_available":kind.is_some() && code.is_some()}))
}

/// Latest means reservation-sequence order, not wall-clock or completion order.
/// Missing/corrupt/unsupported details remain unavailable, never copied verbatim.
pub(super) fn recent(db: &Connection) -> Result<Value> {
    Ok(json!({"available":true,
        "refused":latest(db, State::Refused)?,
        "unknown":latest(db, State::Unknown)?}))
}

#[cfg(test)]
mod tests;
