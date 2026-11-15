//! Advance complete coverage for one archive this writer just published.
//! This never bootstraps a nonempty archive directory or reads old bodies.
use super::*;

pub(in crate::state::archive_catalog) fn catalog() -> Result<Connection> {
    // Establish readiness before opening a writer. In particular a nonempty,
    // unbuilt directory remains incomplete and only schedules background work.
    drop(super::catalog()?);
    let db = open(true, Instant::now() + Duration::from_secs(2))?;
    if ready(&db)? {
        return Ok(db);
    }
    let source = directory()?;
    let empty = source == "absent"
        || fs::read_dir(root().join("archives"))
            .map_err(fail)?
            .next()
            .transpose()
            .map_err(fail)?
            .is_none();
    let occupied: bool = sql(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM archive_files UNION ALL SELECT 1 FROM archive_rows UNION ALL SELECT 1 FROM archive_identities UNION ALL SELECT 1 FROM archive_references LIMIT 1)",
        [],
        |row| row.get(0),
    ))?;
    if !empty || occupied || directory()? != source {
        return Err(INCOMPLETE.into());
    }
    // A proven empty namespace needs no scan. Persist its empty baseline so
    // a sequence of native writes can advance it without a worker interleave.
    let tx = sql(db.unchecked_transaction())?;
    put_meta(&tx, "archive_directory_v2", &source)?;
    for key in [
        "archive_search_errors_v2",
        "archive_reference_errors_v2",
        "archive_identity_errors_v2",
        "archive_files_v2",
    ] {
        put_meta(&tx, key, "0")?;
    }
    put_meta(&tx, "archive_scan_complete_v2", "1")?;
    if directory()? != source {
        return Err(INCOMPLETE.into());
    }
    sql(tx.commit())?;
    Ok(db)
}

#[cfg(all(test, unix))]
std::thread_local! {
    static BEFORE_PROOF: std::cell::Cell<Option<fn()>> = const { std::cell::Cell::new(None) };
}

pub(in crate::state::archive_catalog) fn published(
    db: &Connection,
    path: &Path,
    complete: bool,
) -> Result<()> {
    if !complete || path.parent() != Some(root().join("archives").as_path()) {
        return Err(INCOMPLETE.into());
    }
    let id = path
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or(INCOMPLETE)?;
    let captured = archive_references::reconcile_with_record(db, path)?;
    let identity = NativeIdentity::captured(id, &captured.record)?;
    let row = Row::captured(
        id,
        captured.record,
        captured.stamp.clone(),
        &captured.raw_name,
    )?;
    let row_hash = row.digest()?;
    row.store(db)?;
    NativeIdentity::store(Ok(identity), db, id, &captured.stamp)?;
    let (reference_hash, reference_ok) = match captured.reference_digest {
        Ok(hash) => (hash, true),
        Err(_) => (String::new(), false),
    };
    sql(db.execute(
        "INSERT INTO archive_files VALUES(?,?,?,?,?,?)",
        params![
            id,
            captured.stamp,
            row_hash,
            reference_hash,
            true,
            reference_ok
        ],
    ))?;
    for (key, increment) in [
        ("archive_files_v2", 1),
        ("archive_reference_errors_v2", u64::from(!reference_ok)),
    ] {
        let previous = meta(db, key)?.parse::<u64>().map_err(fail)?;
        let next = previous.checked_add(increment).ok_or(INCOMPLETE)?;
        put_meta(db, key, &next.to_string())?;
    }
    #[cfg(all(test, unix))]
    BEFORE_PROOF.with(|slot| {
        if let Some(hook) = slot.take() {
            hook();
        }
    });
    let source = directory()?;
    put_meta(db, "archive_directory_v2", &source)?;
    let legacy = super::super::stamp(&root().join("archives"))?;
    put_meta(db, "archive_references_v1", "1")?;
    put_meta(db, "archive_references_stamp", &legacy)?;
    put_meta(db, "stamp", &legacy)?;
    // A known write is not a proof that nothing else changed. Recheck the
    // bounded native manifest, including every old file's object stamp.
    if proof(db)? != source || directory()? != source {
        return Err(INCOMPLETE.into());
    }
    sql(db.execute_batch("COMMIT"))
}

#[cfg(all(test, unix))]
mod tests;
