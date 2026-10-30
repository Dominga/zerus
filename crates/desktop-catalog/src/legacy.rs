//! Indexed staging of typed legacy source events. The caller verifies the
//! configured computer and retains its ingress item through each transaction.
//! Complete means coverage only, never freshness, delegation or writer authority.
use super::*;

pub const MAX_ROWS: usize = 250_000;
pub const METADATA_BYTES: usize = watch::PAGE_BYTES;

/// Begin a replacement only after the owned child succeeded. Pages still stay
/// provisional until the caller's full spool replay succeeds and calls complete.
/// Metadata must be the bounded top-level object, without the sessions array.
pub fn begin(
    db: &mut Connection,
    source: &Source,
    metadata: &Value,
    rows: usize,
    pages: usize,
) -> Result<()> {
    validate_source(source)?;
    ensure!(
        metadata.is_object() && metadata.get("sessions").is_none(),
        "invalid legacy metadata"
    );
    let metadata = serde_json::to_string(metadata)?;
    ensure!(
        metadata.len() <= METADATA_BYTES,
        "legacy metadata too large"
    );
    ensure!(
        rows <= MAX_ROWS && pages > 0 && pages <= rows.max(1) && rows <= pages * PAGE_ROWS,
        "invalid legacy replacement counts"
    );
    let tx = db.transaction()?;
    // The existing schema has all required fields. In legacy mode, seq is the
    // expected page count, cursor the expected row count, next_page the received
    // count, and hello the provisional metadata. Empty stream cannot be a valid
    // watch hello. Visible meta is replaced atomically only at completion.
    replace_source(&tx, source, "", pages as i64, &metadata)?;
    let stage = start_generation(&tx, source)?;
    tx.execute(
        "UPDATE desktop_catalog_sources SET staged=?2,next_page=0,cursor=?3 WHERE computer=?1",
        params![source.computer, stage, rows as i64],
    )?;
    tx.commit()?;
    Ok(())
}

/// At most 128 rows and one frame of encoded CatalogItems. Index starts at zero.
/// Every failure retires this exact source, preserving the last-known display.
pub fn page(
    db: &mut Connection,
    source: &Source,
    index: usize,
    items: &[watch::CatalogItem],
) -> Result<Applied> {
    retire_on_error(db, source, |db| {
        ensure!(items.len() <= PAGE_ROWS, "legacy page has too many rows");
        ensure!(
            serde_json::to_vec(items)?.len() <= watch::MAX_FRAME_BYTES,
            "legacy page too large"
        );
        let tx = db.transaction()?;
        let old = legacy_current(&tx, source)?;
        ensure!(
            index as u128 == old.next_page as u128 && old.next_page < old.seq,
            "non-contiguous legacy page"
        );
        ensure!(
            !items.is_empty() || (old.cursor == 0 && old.seq == 1),
            "unexpected empty legacy page"
        );
        let stage = old.staged.context("legacy page without staging")?;
        for item in items {
            let json = row_json(item)?;
            tx.execute(
                "INSERT INTO desktop_catalog_rows(generation,key,json,bytes) VALUES(?1,?2,?3,?4)",
                params![stage, item.key, json, charge(&item.key, &json)],
            )?;
        }
        let rows: i64 = tx.query_row(
            "SELECT rows FROM desktop_catalog_generations WHERE id=?1",
            [stage],
            |r| r.get(0),
        )?;
        ensure!(rows <= old.cursor, "legacy row count exceeds declaration");
        check_budget(&tx)?;
        tx.execute(
            "UPDATE desktop_catalog_sources SET next_page=next_page+1 WHERE computer=?1",
            [&source.computer],
        )?;
        tx.commit()?;
        Ok(Applied::Staging)
    })
}

/// Call only after full spool replay succeeded. An earlier callback is never a
/// final page. Exact declared page and unique-row counts must both match.
pub fn complete(db: &mut Connection, source: &Source) -> Result<Applied> {
    retire_on_error(db, source, |db| {
        let tx = db.transaction()?;
        let old = legacy_current(&tx, source)?;
        let stage = old.staged.context("legacy completion without staging")?;
        let rows: i64 = tx.query_row(
            "SELECT rows FROM desktop_catalog_generations WHERE id=?1",
            [stage],
            |r| r.get(0),
        )?;
        ensure!(
            old.next_page == old.seq && rows == old.cursor,
            "incomplete legacy replacement"
        );
        check_budget(&tx)?;
        publish(&tx, source, &old)?;
        tx.execute(
            "UPDATE desktop_catalog_sources SET meta=hello,hello='{}' WHERE computer=?1",
            [&source.computer],
        )?;
        tx.commit()?;
        Ok(Applied::Visible)
    })
}

fn legacy_current(db: &Connection, source: &Source) -> Result<State> {
    let old = current(db, source)?;
    ensure!(old.stream.is_empty(), "legacy callback for watch source");
    Ok(old)
}
fn retire_on_error(
    db: &mut Connection,
    source: &Source,
    apply: impl FnOnce(&mut Connection) -> Result<Applied>,
) -> Result<Applied> {
    let result = apply(db);
    if result.is_err() {
        let _ = disconnect(db, source);
    }
    result
}

#[cfg(test)]
mod tests;
