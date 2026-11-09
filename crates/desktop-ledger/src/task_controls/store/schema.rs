//! Bounded read-only validation; never rebuild an existing authority ledger.
use super::*;
use std::sync::OnceLock;

const OBJECTS: usize = 32;
const SQL_BYTES: usize = 4096;
type Object = (String, String, String, Option<String>);
static EXPECTED: OnceLock<Vec<Object>> = OnceLock::new();

/// Obtain SQLite's canonical representation of our own DDL once, in memory.
/// This also covers implicit UNIQUE indexes, without duplicating the schema or
/// interpreting arbitrary SQL. The real database is never modified here.
fn expected() -> Result<&'static [Object]> {
    if EXPECTED.get().is_none() {
        let mut db = Connection::open_in_memory()?;
        let tx = db.transaction()?;
        create(&tx)?;
        let objects = objects(&tx)?;
        let _ = EXPECTED.set(objects);
    }
    EXPECTED
        .get()
        .map(Vec::as_slice)
        .context("task-control schema unavailable")
}

fn objects(db: &Connection) -> Result<Vec<Object>> {
    // Include associated autoindexes as well as named tables/indexes/triggers.
    // Bound rows and SQL bytes before SQLite values enter Rust allocations.
    let mut statement = db.prepare(&format!(
        "SELECT CASE WHEN length(CAST(type AS BLOB))<=16 THEN type END,
         CASE WHEN length(CAST(name AS BLOB))<=128 THEN name END,
         CASE WHEN length(CAST(tbl_name AS BLOB))<=128 THEN tbl_name END,
         CASE WHEN length(CAST(sql AS BLOB))<={SQL_BYTES} THEN sql END
         FROM sqlite_schema
         WHERE substr(name,1,21)='desktop_task_control_'
            OR substr(tbl_name,1,21)='desktop_task_control_'
         ORDER BY name LIMIT {}",
        OBJECTS + 1
    ))?;
    let values = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<Vec<Object>>>()?;
    ensure!(
        values.len() <= OBJECTS,
        "unexpected task-control schema objects"
    );
    Ok(values)
}

pub(super) fn validate(tx: &Transaction<'_>) -> Result<()> {
    ensure!(
        objects(tx)?.as_slice() == expected()?,
        "partial or changed task-control schema must be reviewed"
    );
    let mut statement =
        tx.prepare("SELECT id,rows,bytes FROM desktop_task_control_budget LIMIT 2")?;
    let mut rows = statement.query([])?;
    let row = rows
        .next()?
        .context("missing task-control budget singleton")?;
    let id: i64 = row.get(0)?;
    let count: i64 = row.get(1)?;
    let bytes: i64 = row.get(2)?;
    ensure!(
        id == 1
            && (0..=STORE_ROWS as i64).contains(&count)
            && (0..=STORE_BYTES as i64).contains(&bytes),
        "invalid task-control budget singleton"
    );
    ensure!(
        rows.next()?.is_none(),
        "unexpected task-control budget rows"
    );
    Ok(())
}
