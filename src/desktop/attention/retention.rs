//! Bounded compaction of permanently suppressed, never-attempted mappings.
//! Hash-only tombstones outlive payloads: expiration would permit replay of an
//! opaque old edge after Tracker's rolling state forgets it.
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, Transaction};

pub(super) const ROWS: usize = 32;
pub(super) const BYTES: usize = 256 << 10;

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE desktop_attention_tombstones(
        logical BLOB PRIMARY KEY CHECK(length(logical)=32)) WITHOUT ROWID;
      CREATE INDEX desktop_attention_compact ON desktop_attention_edges(blocked,live,id);
      CREATE TABLE desktop_attention_gc(id INTEGER PRIMARY KEY CHECK(id=1),after_id INTEGER NOT NULL,through_id INTEGER NOT NULL);
      INSERT INTO desktop_attention_gc VALUES(1,0,0);
      CREATE TRIGGER desktop_attention_edge_remove AFTER DELETE ON desktop_attention_edges BEGIN
        UPDATE desktop_attention_totals SET edges=edges-1,edge_bytes=edge_bytes-old.bytes WHERE id=1; END;")?;
    Ok(())
}

fn hash(logical: &str) -> Result<[u8; 32]> {
    ensure!(
        logical.len() == 64 && logical.is_ascii(),
        "invalid logical edge hash"
    );
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&logical[index * 2..index * 2 + 2], 16)?;
    }
    Ok(bytes)
}

pub(super) fn suppressed(db: &Connection, logical: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM desktop_attention_tombstones WHERE logical=?1)",
        [hash(logical)?.as_slice()],
        |r| r.get(0),
    )?)
}

#[derive(Debug)]
pub(super) struct Pressure;
impl std::fmt::Display for Pressure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("attention edge quota")
    }
}
impl std::error::Error for Pressure {}

#[derive(Default)]
pub(super) struct Compacted {
    pub scanned: usize,
    pub removed: usize,
    pub bytes: usize,
}

pub(super) fn compact(db: &mut Connection) -> Result<Compacted> {
    let tx = db.transaction()?;
    let (mut after, mut through): (i64, i64) = tx.query_row(
        "SELECT after_id,through_id FROM desktop_attention_gc WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let original_cursor = (after, through);
    if after >= through {
        after = 0;
        through = super::state::head(&tx)?;
    }
    // LIMIT is inside the joined subquery, so protected attempted rows cannot
    // turn one page into an unbounded scan. Only bounded keys/counters cross
    // into Rust; the full payload is never deserialized for GC.
    let mut query = tx.prepare("SELECT e.id,e.logical,e.intent,
        e.bytes+COALESCE(length(CAST(i.identity AS BLOB)),0)+COALESCE(length(CAST(i.payload AS BLOB)),0)+32,
        (i.id IS NULL OR i.blocked=1) AND NOT EXISTS(SELECT 1 FROM desktop_attempts a WHERE a.intent_id=e.intent)
        FROM (SELECT id,logical,intent,bytes FROM desktop_attention_edges INDEXED BY desktop_attention_compact
          WHERE blocked=1 AND live=0 AND id>?1 AND id<=?2 ORDER BY id LIMIT ?3) e
        LEFT JOIN desktop_intents i ON i.id=e.intent ORDER BY e.id")?;
    let rows = query
        .query_map(params![after, through, ROWS as i64], |r| {
            let logical = r
                .get_ref(1)?
                .as_str()
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let intent = r
                .get_ref(2)?
                .as_str()
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            if logical.len() != 64 || intent.len() != 64 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok((
                r.get::<_, i64>(0)?,
                logical.to_owned(),
                intent.to_owned(),
                r.get::<_, i64>(3)?,
                r.get::<_, bool>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(query);
    let mut result = Compacted {
        scanned: rows.len(),
        ..Compacted::default()
    };
    if rows.is_empty() {
        after = through;
    }
    for (id, logical, intent, charge, eligible) in rows {
        if eligible && (1..=BYTES as i64).contains(&charge) {
            let charge = charge as usize;
            if result.bytes + charge > BYTES {
                break;
            }
            // The hash, both deletions, totals and cursor are one commit. A
            // crash cannot delete suppression evidence before its replacement.
            tx.execute(
                "INSERT OR IGNORE INTO desktop_attention_tombstones(logical) VALUES(?1)",
                [hash(&logical)?.as_slice()],
            )?;
            tx.execute("DELETE FROM desktop_attention_edges WHERE id=?1", [id])?;
            // A shared intent remains until its last mapping has compacted.
            tx.execute(
                "DELETE FROM desktop_intents WHERE id=?1 AND blocked=1
                AND NOT EXISTS(SELECT 1 FROM desktop_attempts WHERE intent_id=?1)
                AND NOT EXISTS(SELECT 1 FROM desktop_attention_edges WHERE intent=?1)",
                [&intent],
            )?;
            result.removed += 1;
            result.bytes += charge;
        }
        after = id;
    }
    if (after, through) != original_cursor {
        tx.execute(
            "UPDATE desktop_attention_gc SET after_id=?1,through_id=?2 WHERE id=1",
            params![after, through],
        )?;
    }
    tx.commit()?;
    Ok(result)
}
